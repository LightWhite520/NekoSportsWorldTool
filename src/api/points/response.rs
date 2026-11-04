//! Bounded checkpoint decoding, adapted from GAN and Demacia's response handling.
//! Only named business wrappers are traversed; fence vertices are never checkpoints.
use serde_json::Value;

const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 8;
const LIST_FIELDS: [&str; 3] = ["pointsResModels", "pointResModels", "pointsModelList"];

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResponseContext {
    pub list_path: String,
    /// Only the explicitly named runAreaId, never id/areaId/runId.
    pub run_area_id: Option<i64>,
    pub geo_fences: Option<Value>,
    pub freedom_show_fence: Option<bool>,
}

#[derive(Debug, PartialEq)]
pub enum PointResponse {
    Points(Vec<Value>, ResponseContext),
    Empty,
    NotConfigured,
}

impl PointResponse {
    pub(super) fn require_points(self) -> Result<(Vec<Value>, ResponseContext), String> {
        match self {
            Self::Points(points, context) => Ok((points, context)),
            Self::Empty => {
                Err("点位接口成功返回空列表，不能据此确定原因；请在官方 App 核对当前任务".into())
            }
            Self::NotConfigured => {
                Err("服务器明确提示学校尚未配置检查点，请联系学校老师确认".into())
            }
        }
    }
}

pub(super) fn parse(payload: &Value) -> Result<PointResponse, String> {
    let message = payload
        .get("message")
        .or_else(|| payload.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if payload.get("error").and_then(Value::as_i64) == Some(10600)
        && matches!(
            message,
            "当前学校尚未设置点位，请联系学校老师" | "当前学校尚未设置点位，请联系学校老师。"
        )
    {
        return Ok(PointResponse::NotConfigured);
    }
    // Validate the business result before inspecting any list, including nested ones.
    super::super::client::check_business(payload)?;
    let mut budget = MAX_JSON_BYTES;
    let found = find(payload, "$", 0, &mut budget, ResponseContext::default())?
        .ok_or("点位接口缺少明确的检查点列表字段")?;
    if found.0.is_empty() {
        return Ok(PointResponse::Empty);
    }
    if super::points_bd(&found.0).len() != found.0.len() {
        return Err("点位接口含无效坐标，未丢弃或替换任何检查点".into());
    }
    Ok(PointResponse::Points(found.0, found.1))
}

fn decode(value: &Value, budget: &mut usize) -> Result<Value, String> {
    if let Value::String(text) = value {
        *budget = budget
            .checked_sub(text.len())
            .ok_or("点位响应 JSON 超过 8 MiB")?;
        serde_json::from_str(text).map_err(|_| "点位响应 JSON 格式无效".into())
    } else {
        Ok(value.clone())
    }
}

fn find(
    value: &Value,
    path: &str,
    depth: usize,
    budget: &mut usize,
    mut context: ResponseContext,
) -> Result<Option<(Vec<Value>, ResponseContext)>, String> {
    if depth > MAX_DEPTH {
        return Err("点位响应嵌套层数过多".into());
    }
    let value = decode(value, budget)?;
    let object = value.as_object().ok_or("点位业务包装不是对象")?;
    if let Some(error) = object.get("error").and_then(Value::as_i64) {
        if error != 10000 {
            return Err(format!("点位嵌套响应业务错误 {error}"));
        }
    }
    if let Some(id) = object.get("runAreaId").filter(|v| !v.is_null()) {
        let id = id
            .as_i64()
            .or_else(|| id.as_str()?.trim().parse().ok())
            .ok_or("点位响应 runAreaId 格式无效")?;
        if id >= 0 {
            if context.run_area_id.is_some_and(|previous| previous != id) {
                return Err("点位响应包含冲突的区域 ID，无法确认所属区域".into());
            }
            context.run_area_id = Some(id);
        }
    }
    if let Some(fences) = object.get("geoFences").filter(|v| !v.is_null()) {
        let fences = decode(fences, budget)?;
        if !fences.is_array() {
            return Err("点位响应 geoFences 不是列表".into());
        }
        context.geo_fences = Some(fences);
    }
    if let Some(show) = object.get("freedomShowFence").and_then(Value::as_bool) {
        context.freedom_show_fence = Some(show);
    }
    let mut selected: Option<(Vec<Value>, ResponseContext)> = None;
    for field in LIST_FIELDS {
        if let Some(raw) = object.get(field).filter(|v| !v.is_null()) {
            let decoded = decode(raw, budget)?;
            let points = decoded.as_array().ok_or("检查点列表格式无效")?.clone();
            let mut source = context.clone();
            source.list_path = format!("{path}.{field}");
            merge(&mut selected, (points, source))?;
        }
    }
    // Preserve existing top-level precedence. Never search arbitrary objects or arrays.
    if selected.is_some() {
        return Ok(selected);
    }
    for wrapper in ["data", "result"] {
        if let Some(child) = object.get(wrapper).filter(|v| !v.is_null()) {
            if let Some(found) = find(
                child,
                &format!("{path}.{wrapper}"),
                depth + 1,
                budget,
                context.clone(),
            )? {
                merge(&mut selected, found)?;
            }
        }
    }
    Ok(selected)
}

fn merge(
    selected: &mut Option<(Vec<Value>, ResponseContext)>,
    found: (Vec<Value>, ResponseContext),
) -> Result<(), String> {
    if let Some(previous) = selected {
        if previous.0 != found.0
            || previous.1.run_area_id != found.1.run_area_id
            || previous.1.geo_fences != found.1.geo_fences
            || previous.1.freedom_show_fence != found.1.freedom_show_fence
        {
            return Err("点位响应包含多个不同列表或区域，不能自动选择".into());
        }
    } else {
        *selected = Some(found);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn point() -> Value {
        json!({"lat":"39.9", "lon":"116.4", "isFixed":0})
    }

    #[test]
    fn nested_encoded_alias_preserves_points_and_explicit_context() {
        for field in LIST_FIELDS {
            let mut inner = json!({"runAreaId":"42", "geoFences":[{"id":73,"points":[]}], "freedomShowFence":true});
            inner[field] = json!(vec![point()]).to_string().into();
            let payload =
                json!({"error":10000,"data":json!({"result":inner.to_string()}).to_string()});
            let (points, context) = parse(&payload).unwrap().require_points().unwrap();
            assert_eq!(points, vec![point()]);
            assert_eq!(context.run_area_id, Some(42));
            assert_eq!(context.list_path, format!("$.data.result.{field}"));
            assert_eq!(context.geo_fences.unwrap()[0]["id"], 73);
        }
    }

    #[test]
    fn fence_vertices_and_generic_ids_are_never_inferred() {
        let only_fence = json!({"error":10000,"data":{"geoFences":[{"points":[point()]}]}});
        assert!(parse(&only_fence).is_err());
        let payload =
            json!({"error":10000,"areaId":42,"runId":42,"id":42,"pointsResModels":[point()]});
        assert_eq!(
            parse(&payload)
                .unwrap()
                .require_points()
                .unwrap()
                .1
                .run_area_id,
            None
        );
    }

    #[test]
    fn empty_unconfigured_errors_and_bad_data_are_distinct() {
        assert_eq!(
            parse(&json!({"error":10000,"pointsResModels":[]})).unwrap(),
            PointResponse::Empty
        );
        assert_eq!(
            parse(&json!({"error":10600,"message":"当前学校尚未设置点位，请联系学校老师"}))
                .unwrap(),
            PointResponse::NotConfigured
        );
        for payload in [
            json!({"error":10600,"message":"其他错误"}),
            json!({"error":10603,"pointsResModels":[point()]}),
            json!({"error":10000,"data":{"error":10603,"pointsResModels":[point()]}}),
            json!({"error":10000,"pointsResModels":null}),
            json!({"error":10000,"pointsResModels":[{}]}),
            json!({"error":10000,"pointsResModels":"broken PRIVATE"}),
        ] {
            let error = parse(&payload).unwrap_err();
            assert!(!error.contains("PRIVATE"));
        }
    }

    #[test]
    fn precedence_and_ambiguity_are_explicit() {
        let top = json!({"error":10000,"pointsResModels":[point()],"data":{"pointsResModels":[]}});
        assert_eq!(
            parse(&top).unwrap().require_points().unwrap().0,
            vec![point()]
        );
        for payload in [
            json!({"error":10000,"pointsResModels":[point()],"pointResModels":[]}),
            json!({"error":10000,"data":{"pointsResModels":[point()]},"result":{"pointsResModels":[]}}),
            json!({"error":10000,"runAreaId":42,"data":{"runAreaId":43,"pointsResModels":[point()]}}),
        ] {
            assert!(parse(&payload).is_err());
        }
    }

    #[test]
    fn decoding_is_bounded() {
        let mut nested = json!({"pointsResModels":[point()]});
        for _ in 0..10 {
            nested = json!({"data":nested});
        }
        assert!(parse(&json!({"error":10000,"data":nested}))
            .unwrap_err()
            .contains("层数"));
        assert!(
            parse(&json!({"error":10000,"pointsResModels":" ".repeat(MAX_JSON_BYTES+1)}))
                .unwrap_err()
                .contains("8 MiB")
        );
    }
}
