use pyo3::prelude::*;
use serde_json::json;
use tracing_core::{span, Event, Subscriber};
use tracing_serde::AsSerde;
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

/// `PythonCallbackLayerBridge` is an adapter allowing the
/// [`tracing_subscriber::layer::Layer`] trait to be implemented by a Python
/// object. Each trait method's arguments are serialized as JSON strings and
/// passed to the corresponding method on the Python object if it exists.
///
/// The interface `PythonCallbackLayerBridge` expects Python objects to
/// implement differs slightly from the `Layer` trait in Rust:
/// - The Python implementation of `on_new_span` may return some state that will
///   be stored in the new span's [`tracing_subscriber::registry::Extensions`].
/// - When calling other trait methods, `PythonCallbackLayerBridge` will get
///   that state from the current span and pass it back to Python as an
///   additional positional argument.
///
/// The state is opaque to `PythonCallbackLayerBridge` but, for example, a layer
/// for a Python tracing system could create a Python span for each Rust span
/// and use a reference to the Python span as the state.
///
/// Currently only a subset of `Layer` methods are bridged to Python:
/// - [`tracing_subscriber::layer::Layer::on_event`], with corresponding Python
///   signature ```python def on_event(self, event: str, state: Any): ... ```
/// - [`tracing_subscriber::layer::Layer::on_new_span`] ```python def
///   on_new_span(self, span_attrs: str, span_id: str): ... ```
/// - [`tracing_subscriber::layer::Layer::on_close`] ```python def
///   on_close(self, span_id: str, state: Any): ... ```
/// - [`tracing_subscriber::layer::Layer::on_record`] ```python def
///   on_record(self, span_id: str, values: str, state: Any): ... ```
pub struct PythonCallbackLayerBridge {
    on_event: Option<Py<PyAny>>,
    on_new_span: Option<Py<PyAny>>,
    on_close: Option<Py<PyAny>>,
    on_record: Option<Py<PyAny>>,
}

impl PythonCallbackLayerBridge {
    pub fn new(py_impl: Bound<'_, PyAny>) -> PythonCallbackLayerBridge {
        let on_event = py_impl.getattr("on_event").ok().map(Bound::unbind);
        let on_close = py_impl.getattr("on_close").ok().map(Bound::unbind);
        let on_new_span = py_impl.getattr("on_new_span").ok().map(Bound::unbind);
        let on_record = py_impl.getattr("on_record").ok().map(Bound::unbind);

        PythonCallbackLayerBridge {
            on_event,
            on_close,
            on_new_span,
            on_record,
        }
    }
}

impl<S> Layer<S> for PythonCallbackLayerBridge
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event, ctx: Context<'_, S>) {
        let Some(py_on_event) = &self.on_event else {
            return;
        };

        let current_span = event
            .parent()
            .and_then(|id| ctx.span(id))
            .or_else(|| ctx.lookup_current());
        let extensions = current_span.as_ref().map(|span| span.extensions());
        let json_event = json!(event.as_serde()).to_string();

        Python::with_gil(|py| {
            let py_state =
                extensions.map(|ext| ext.get::<Py<PyAny>>().map(|state| state.clone_ref(py)));
            let _ = py_on_event.bind(py).call((json_event, py_state), None);
        })
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, span_id: &span::Id, ctx: Context<'_, S>) {
        let (Some(py_on_new_span), Some(current_span)) = (&self.on_new_span, ctx.span(span_id))
        else {
            return;
        };

        let json_attrs = json!(attrs.as_serde()).to_string();
        let json_id = json!(span_id.as_serde()).to_string();
        let mut extensions = current_span.extensions_mut();

        Python::with_gil(|py| {
            let Ok(py_state) = py_on_new_span.bind(py).call((json_attrs, json_id), None) else {
                return;
            };

            extensions.insert::<Py<PyAny>>(py_state.unbind());
        })
    }

    fn on_close(&self, span_id: span::Id, ctx: Context<'_, S>) {
        let (Some(py_on_close), Some(current_span)) = (&self.on_close, ctx.span(&span_id)) else {
            return;
        };

        let json_id = json!(span_id.as_serde()).to_string();
        let py_state = current_span.extensions_mut().remove::<Py<PyAny>>();

        Python::with_gil(|py| {
            let _ = py_on_close.bind(py).call((json_id, py_state), None);
        })
    }

    fn on_record(&self, span_id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let (Some(py_on_record), Some(current_span)) = (&self.on_record, ctx.span(span_id)) else {
            return;
        };

        let json_id = json!(span_id.as_serde()).to_string();
        let json_values = json!(values.as_serde()).to_string();
        let extensions = current_span.extensions();

        Python::with_gil(|py| {
            let py_state = extensions
                .get::<Py<PyAny>>()
                .map(|state| state.clone_ref(py));

            let _ = py_on_record
                .bind(py)
                .call((json_id, json_values, py_state), None);
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{ops::RangeFrom, sync::Once};

    use serde_json::{Map, Value};
    use tracing::{info, instrument, warn_span};
    use tracing_subscriber::prelude::*;

    use super::*;

    static INIT: Once = Once::new();

    #[pyclass]
    struct PythonLayer {
        span_ids: RangeFrom<u16>,
        pub events: Vec<(String, String, u16)>,
        pub new_spans: Vec<Value>,
        pub closed_spans: Vec<u16>,
        pub span_records: Vec<(Value, u16)>,
    }

    #[pymethods]
    impl PythonLayer {
        #[new]
        pub fn new() -> PythonLayer {
            PythonLayer {
                span_ids: 0..,
                events: Vec::new(),
                new_spans: Vec::new(),
                closed_spans: Vec::new(),
                span_records: Vec::new(),
            }
        }

        pub fn on_event(&mut self, event: String, state: u16) {
            let event = serde_json::from_str::<Map<String, Value>>(&event).unwrap();
            let message = event.get("message").unwrap().as_str().unwrap();
            let level = event
                .get("metadata")
                .unwrap()
                .get("level")
                .unwrap()
                .as_str()
                .unwrap();

            self.events
                .push((message.to_owned(), level.to_owned(), state));
        }

        pub fn on_new_span(&mut self, span_attrs: String, _span_id: String) -> u16 {
            let span_attrs = serde_json::from_str::<Map<String, Value>>(&span_attrs).unwrap();
            let metadata = span_attrs.get("metadata").unwrap().as_object().unwrap();

            let mut stripped_attrs = Map::new();

            stripped_attrs.insert("level".to_string(), metadata.get("level").unwrap().clone());
            stripped_attrs.insert("name".to_string(), metadata.get("name").unwrap().clone());

            let fields = metadata.get("fields").unwrap().as_array().unwrap();
            for field in fields {
                let field = field.as_str().unwrap();
                if let Some(value) = span_attrs.get(field) {
                    stripped_attrs.insert(field.to_owned(), value.clone());
                }
            }

            self.new_spans.push(stripped_attrs.into());
            self.span_ids.next().unwrap()
        }

        pub fn on_close(&mut self, _span_id: String, state: u16) {
            self.closed_spans.push(state);
        }

        pub fn on_record(&mut self, _span_id: String, values: String, state: u16) {
            let values = serde_json::from_str(&values).unwrap();
            self.span_records.push((values, state));
        }
    }

    fn initialize_tracing() -> (Py<PythonLayer>, tracing::dispatcher::DefaultGuard) {
        INIT.call_once(|| {
            pyo3::prepare_freethreaded_python();
        });
        let (py_layer, rs_layer) = Python::with_gil(|py| {
            let py_layer = Bound::new(py, PythonLayer::new()).unwrap();
            let (py_layer, py_layer_unbound) = (py_layer.clone().into_any(), py_layer.unbind());
            (py_layer_unbound, PythonCallbackLayerBridge::new(py_layer))
        });
        (
            py_layer,
            tracing_subscriber::registry().with(rs_layer).set_default(),
        )
    }

    #[instrument(fields(data))]
    fn func(arg1: u16, arg2: String) {
        info!("About to record something");
        tracing::Span::current().record("data", "some data");
    }

    #[test]
    fn test_simple_span() {
        let (py_layer, _dispatcher) = initialize_tracing();

        func(1337, "foo".to_string());

        let expected_events = vec![("About to record something".to_owned(), "INFO".to_owned(), 0)];
        let expected_new_spans =
            vec![json!({"arg1": 1337, "arg2": "\"foo\"", "level": "INFO", "name": "func"})];
        let expected_closed_spans = vec![0];
        let expected_records = vec![(json!({"data": "some data"}), 0)];

        Python::with_gil(|py| {
            let borrowed = py_layer.borrow(py);
            assert_eq!(&expected_events, &borrowed.events);
            assert_eq!(&expected_new_spans, &borrowed.new_spans);
            assert_eq!(&expected_closed_spans, &borrowed.closed_spans);
            assert_eq!(&expected_records, &borrowed.span_records);
        });
    }

    #[test]
    fn test_nested_span() {
        let (py_layer, _dispatcher) = initialize_tracing();

        {
            let span = warn_span!("outer");
            span.in_scope(|| {
                func(1337, "bar".to_string());
            });
        }

        let expected_events = vec![("About to record something".to_owned(), "INFO".to_owned(), 1)];
        let expected_new_spans = vec![
            json!({"level": "WARN", "name": "outer"}),
            json!({"arg1": 1337, "arg2": "\"bar\"", "level": "INFO", "name": "func"}),
        ];
        let expected_closed_spans = vec![1, 0];
        let expected_records = vec![(json!({"data": "some data"}), 1)];

        Python::with_gil(|py| {
            let borrowed = py_layer.borrow(py);
            assert_eq!(&expected_events, &borrowed.events);
            assert_eq!(&expected_new_spans, &borrowed.new_spans);
            assert_eq!(&expected_closed_spans, &borrowed.closed_spans);
            assert_eq!(&expected_records, &borrowed.span_records);
        });
    }
}
