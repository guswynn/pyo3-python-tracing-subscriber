import itertools
import json

from demo_rust_extension import fibonacci, initialize_tracing

class DemoTracingLayer:
    def __init__(self):
        self.span_ids = itertools.count()
        self.span_map = {}

    def on_event(self, event: str, state: int):
        rust_span_id = self.span_map[state]
        print(f"[on_event]: Event in span ({state}, {rust_span_id}) | {event}")

    def on_new_span(self, span_attrs: str, span_id: str) -> int:
        python_span_id = next(self.span_ids)
        self.span_map[python_span_id] = span_id
        print(f"[on_new_span]: New span ({python_span_id}, {span_id}) | {span_attrs}")
        return python_span_id

    def on_close(self, span_id: str, state: int):
        print(f"[on_close]: Closing span ({state}, {span_id})")

    def on_record(self, span_id: str, values: str, state: int):
        values = json.loads(values)
        print(f"[on_record]: Value recorded in ({state}, {span_id}) | {values}")


def main():
    initialize_tracing(DemoTracingLayer())

    print("Calling memoized fibonacci generator")
    print(f"PYTHON: fibonacci(10, True) {fibonacci(10, True)}")

    print("Calling naive fibonacci generator")
    print(f"PYTHON: fibonacci(10, False) {fibonacci(10, False)}")

    print("(Intentional error) Calling naive fibonacci generator with too big an index")
    print(f"PYTHON: fibonacci(30, False) {fibonacci(30, False)}")

if __name__ == "__main__":
    main()
