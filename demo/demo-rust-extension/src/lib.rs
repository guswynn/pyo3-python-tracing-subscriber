use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tracing::{debug, error, info, trace, warn};
use tracing_subscriber::prelude::*;

#[tracing::instrument]
fn naive_fibonacci(index: usize) -> usize {
    debug!("Getting the {}th fibonacci number", index);
    if index == 0 || index == 1 {
        trace!("Base case: {}", index);
        1
    } else {
        trace!(
            "Calling recursively to get sum of {} and {}",
            index - 1,
            index - 2
        );
        naive_fibonacci(index - 1) + naive_fibonacci(index - 2)
    }
}

#[tracing::instrument]
fn memoized_fibonacci(index: usize) -> usize {
    debug!("Getting the {}th fibonacci number", index);
    if index == 0 || index == 1 {
        trace!("Base case: {}", index);
        return 1;
    }
    let mut memo = Vec::with_capacity(index);
    memo.push(1);
    memo.push(1);

    for i in 2..=index {
        trace!("Memoizing {} by adding {} and {}", i, i - 1, i - 2);
        memo.push(memo[i - 1] + memo[i - 2]);
    }

    memo[index]
}

#[tracing::instrument(fields(version))]
#[pyfunction]
fn fibonacci(index: usize, use_memoized: bool) -> PyResult<usize> {
    if use_memoized {
        info!("Using memoized fibonacci generator");
        tracing::Span::current().record("version", "memoized");
        Ok(memoized_fibonacci(index))
    } else if index <= 15 {
        warn!("Warning: using the naive fibonacci generator");
        tracing::Span::current().record("version", "naive");
        Ok(naive_fibonacci(index))
    } else {
        error!(
            "Error: using the naive fibonacci generator with too high an index: {}",
            index
        );
        Err(PyRuntimeError::new_err(
            "index too high for naive fibonacci generator",
        ))
    }
}

#[pyfunction]
pub fn initialize_tracing(py_impl: Bound<'_, PyAny>) {
    tracing_subscriber::registry()
        .with(pyo3_python_tracing_subscriber::PythonCallbackLayerBridge::new(py_impl))
        .init();
}

#[pymodule]
fn _bindings(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fibonacci, m)?)?;
    m.add_function(wrap_pyfunction!(initialize_tracing, m)?)?;

    Ok(())
}
