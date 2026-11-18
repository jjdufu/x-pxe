//! minijinja 环境：注册 `sh_quote` 并渲染内嵌模板。

use minijinja::value::Serde;
use minijinja::{Environment, Error as JinjaError, Value};
use serde::Serialize;

use crate::RenderError;
use crate::filters::sh_quote;

/// 渲染命名模板。
///
/// # Errors
///
/// 模板语法或渲染失败时返回错误。
pub(crate) fn render_named<C: Serialize>(
    name: &str,
    source: &str,
    context: &C,
) -> Result<String, RenderError> {
    let mut env = Environment::new();
    env.add_filter("sh_quote", jinja_sh_quote);
    env.add_template(name, source)
        .map_err(|error| jinja_error("template_parse", &error))?;
    let template = env
        .get_template(name)
        .map_err(|error| jinja_error("template_missing", &error))?;
    template
        .render(Serde(context))
        .map_err(|error| jinja_error("template_render", &error))
}

/// minijinja 过滤器入口。
fn jinja_sh_quote(value: &Value) -> String {
    sh_quote(&value.to_string())
}

/// 包装模板错误。
fn jinja_error(code: &'static str, error: &JinjaError) -> RenderError {
    RenderError::new(code, error.to_string())
}
