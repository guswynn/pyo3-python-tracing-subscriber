This folder contains a demo project that uses `pyo3-python-tracing-subscriber`. To run it, run the following:
```
python -m venv .venv
source .venv/bin/activate
pip install -r demo-py-project/requirements.in

python demo-py-project/main.py
```

The `demo-rust-extension` folder contains a native extension built with `maturin`. It exposes two Rust functions to Python:
- `initialize_tracing()`, which takes a Python implementation of `Layer`
- `fibonacci()`, a Rust function with obnoxiously thorough instrumentation

The `demo-py-project` folder contains a Python file (`main.py`). It contains a `Layer` implementation, which is instantiates and passes into `initialize_tracing()`, and a `main()` function that calls `fibonacci()` a few times. The `Layer` implementation prints its arguments and also associates a separate "Python span ID" with each span to illustrate how Python state is plumbed in Rust.
