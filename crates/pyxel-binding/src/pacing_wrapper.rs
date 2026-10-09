// [pyxel-ne] Display pacing API: vsync and vsync_enabled. Desktop only: in a page vsync stays off.

use pyo3::prelude::*;
use pyxel::Pyxel;

use crate::pyxel_singleton::pyxel;

#[pyfunction]
fn vsync(on: bool) -> i32 {
    pyxel().set_vsync(on)
}

#[pyfunction]
fn vsync_enabled() -> bool {
    Pyxel::vsync_enabled()
}

pub fn add_pacing_functions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(vsync, m)?)?;
    m.add_function(wrap_pyfunction!(vsync_enabled, m)?)?;
    m.add("NE_PACING", true)?;
    Ok(())
}
