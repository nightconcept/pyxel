// [pyxel-ne] User screen shader API: screen_shader, shader_uniform, shader_image,
// plus display_capture to save what the shader drew.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::image_wrapper::Image;
use crate::pyxel_singleton::pyxel;

#[pyfunction]
#[pyo3(signature = (code=None))]
fn screen_shader(code: Option<&str>) -> PyResult<()> {
    pyxel().set_screen_shader(code).map_err(PyValueError::new_err)
}

#[pyfunction]
fn shader_uniform(name: &str, values: Bound<'_, PyAny>) -> PyResult<()> {
    let values: Vec<f32> = match values.extract::<f32>() {
        Ok(value) => vec![value],
        Err(_) => values
            .extract()
            .map_err(|_| PyValueError::new_err("values must be a float or a sequence of floats"))?,
    };
    pyxel()
        .set_shader_uniform(name, &values)
        .map_err(PyValueError::new_err)
}

#[pyfunction]
#[pyo3(signature = (slot, img=None))]
fn shader_image(slot: u32, img: Option<Image>) -> PyResult<()> {
    pyxel()
        .set_shader_image(slot, img.map(|img| img.inner))
        .map_err(PyValueError::new_err)
}

#[pyfunction]
#[pyo3(signature = (filename, scale=1))]
fn display_capture(filename: &str, scale: u32) {
    pyxel().request_display_capture(filename, scale);
}

pub fn add_shader_functions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(screen_shader, m)?)?;
    m.add_function(wrap_pyfunction!(shader_uniform, m)?)?;
    m.add_function(wrap_pyfunction!(shader_image, m)?)?;
    m.add_function(wrap_pyfunction!(display_capture, m)?)?;
    m.add("NUM_SHADER_IMAGES", pyxel::NUM_SHADER_IMAGES)?;
    m.add("NE", true)?;
    Ok(())
}
