//! Only server-provided campus metadata and polygons are selectable.
use super::fence::GeoFenceResponse;
use sha2::{Digest, Sha256};

/// Policy runAreaModels.id is a run-area ID; geoFences.id is only a fence ID.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RunArea {
    pub id: i64,
    pub name: String,
    pub valid: bool,
    pub fence_ids: Vec<i64>,
}

pub fn parse_run_areas(value: Option<&serde_json::Value>) -> Result<Vec<RunArea>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(vec![]); };
    let entries = value.as_array().ok_or("策略 runAreaModels 不是列表")?;
    let mut areas: Vec<RunArea> = Vec::new();
    for entry in entries {
        let id = entry.get("id").and_then(super::fence::run_area_id)
            .ok_or("策略跑区缺少有效 id")?;
        if areas.iter().any(|a| a.id == id) {
            return Err("策略包含重复跑区 ID，无法确定配置".into());
        }
        let valid = entry.get("valid").and_then(serde_json::Value::as_bool)
            .ok_or("策略跑区缺少有效 valid 状态")?;
        let name = entry.get("name").and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty()).ok_or("策略跑区缺少名称")?.to_owned();
        let refs = entry.get("fenceIds").and_then(serde_json::Value::as_array)
            .filter(|v| !v.is_empty()).ok_or("策略跑区缺少围栏关联 fenceIds")?;
        let mut fence_ids = Vec::new();
        for v in refs {
            let fence_id = super::fence::run_area_id(v).ok_or("跑区 fenceIds 包含无效 ID")?;
            if !fence_ids.contains(&fence_id) { fence_ids.push(fence_id); }
        }
        areas.push(RunArea { id, name, valid, fence_ids });
    }
    Ok(areas)
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Campus {
    pub key: String,
    pub name: String,
    pub polygons: Vec<Vec<(f64, f64)>>,
    pub fences: Vec<serde_json::Value>,
    #[serde(default)]
    pub run_area: Option<RunArea>,
}
impl Campus {
    /// Only explicit server area IDs may scope the points request; fence IDs are unrelated.
    pub fn run_area_id(&self, fallback: i64) -> Result<Option<i64>, String> {
        if let Some(area) = &self.run_area {
            if !area.valid {
                return Err("所选跑区当前无效，请刷新并重新选择有效跑区".into());
            }
            return Ok(Some(area.id));
        }
        let mut ids = Vec::new();
        for fence in &self.fences {
            let Some(value) = fence.get("runAreaId").filter(|v| !v.is_null()) else {
                continue;
            };
            let id = value
                .as_i64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
                .ok_or("校区 runAreaId 格式无效")?;
            if id >= 0 && !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() > 1 {
            return Err("所选校区包含多个不同区域 ID，不能确定点位查询范围".into());
        }
        Ok(ids
            .first()
            .copied()
            .or((fallback >= 0).then_some(fallback)))
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    pub school: String,
    pub key: String,
    pub confirmed: bool,
}
fn field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}
pub fn catalog(response: &GeoFenceResponse) -> Vec<Campus> {
    let mut grouped: Vec<(String, Campus)> = Vec::new();
    for (i, (poly, fence)) in response
        .polygons
        .iter()
        .zip(&response.area.geo_fences)
        .enumerate()
    {
        let group = field(fence, "campusId")
            .map(|v| format!("campus:{v}"))
            .unwrap_or_else(|| format!("fence:{i}"));
        if let Some((_, campus)) = grouped.iter_mut().find(|(g, _)| *g == group) {
            campus.polygons.push(poly.clone());
            campus.fences.push(fence.clone());
            continue;
        }
        let name = field(fence, "campusName")
            .or_else(|| {
                field(fence, "fenceName")
                    .or_else(|| field(fence, "name"))
                    .map(|n| format!("围栏：{n}"))
            })
            .unwrap_or_else(|| format!("服务端围栏 {}（未提供校区名）", i + 1));
        grouped.push((
            group,
            Campus {
                key: String::new(),
                name,
                polygons: vec![poly.clone()],
                fences: vec![fence.clone()],
                run_area: None,
            },
        ));
    }
    grouped
        .into_iter()
        .map(|(group, mut c)| {
            c.key = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(group, &c.polygons)).unwrap())
            );
            c
        })
        .collect()
}

/// Use the policy's explicit area-to-fence relation when present. Never guess
/// from names, proximity or a fence's own ID. Legacy schools keep their catalog.
pub fn catalog_with_areas(response: &GeoFenceResponse, areas: &[RunArea]) -> Result<Vec<Campus>, String> {
    if areas.is_empty() { return Ok(catalog(response)); }
    let mut result = Vec::new();
    for area in areas {
        let mut c = Campus {
            key: String::new(), name: area.name.clone(), polygons: vec![], fences: vec![],
            run_area: Some(area.clone()),
        };
        for id in &area.fence_ids {
            let matches: Vec<_> = response.area.geo_fences.iter().enumerate()
                .filter(|(_, f)| f.get("id").and_then(super::fence::run_area_id) == Some(*id))
                .collect();
            if matches.len() != 1 {
                return Err("跑区关联的围栏缺失或 ID 重复，请刷新学校配置".into());
            }
            let (index, fence) = matches[0];
            if let Some(explicit) = fence.get("runAreaId").filter(|v| !v.is_null()) {
                if super::fence::run_area_id(explicit) != Some(area.id) {
                    return Err("策略跑区与围栏 runAreaId 冲突，请刷新学校配置".into());
                }
            }
            c.polygons.push(response.polygons.get(index).ok_or("跑区围栏几何缺失")?.clone());
            c.fences.push(fence.clone());
        }
        // Include the declared area and fence IDs: an unchanged polygon does
        // not authorize silently switching to a different server area.
        c.key = format!("{:x}", Sha256::digest(serde_json::to_vec(
            &("policy-run-area", area.id, &area.fence_ids, &c.polygons)
        ).unwrap()));
        result.push(c);
    }
    Ok(result)
}
pub fn chosen<'a>(
    list: &'a [Campus],
    selection: Option<&Selection>,
    school: &str,
) -> Option<&'a Campus> {
    selection
        .filter(|s| s.school == school)
        .and_then(|s| list.iter().find(|c| c.key == s.key))
}
pub fn selected_response(
    response: GeoFenceResponse,
    areas: &[RunArea],
    selection: Option<&Selection>,
    school: &str,
) -> Result<GeoFenceResponse, String> {
    let list = catalog_with_areas(&response, areas)?;
    let c = chosen(&list, selection, school)
        .filter(|_| selection.map(|s| s.confirmed).unwrap_or(false))
        .ok_or("请到路网页选择校区或跑区并确认围栏；区域配置变化后需要重新确认")?;
    let mut area = response.area;
    area.geo_fences = c.fences.clone();
    area.freedom_show_fence = true;
    area.run_area_id = c
        .run_area_id(area.run_area_id)?
        .unwrap_or(-1);
    Ok(GeoFenceResponse {
        polygons: c.polygons.clone(),
        area,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_confirmation_are_bound_to_real_fences_and_school() {
        let mut r = GeoFenceResponse::default();
        r.polygons = vec![
            vec![(1., 1.), (1., 2.), (2., 1.)],
            vec![(3., 3.), (3., 4.), (4., 3.)],
        ];
        r.area.geo_fences = vec![
            serde_json::json!({"campusId":1,"campusName":"东校区"}),
            serde_json::json!({"id":9}),
        ];
        let c = catalog(&r);
        assert_eq!(c[0].name, "东校区");
        assert!(c[1].name.contains("未提供"));
        let sel = Selection {
            school: "a".into(),
            key: c[0].key.clone(),
            confirmed: true,
        };
        assert!(chosen(&c, Some(&sel), "b").is_none());
        let picked = selected_response(r, &[], Some(&sel), "a").unwrap();
        assert_eq!(picked.polygons.len(), 1);
        assert_eq!(picked.area.run_area_id, -1);
    }
}

#[cfg(test)]
mod area_tests {
    use super::*;
    use serde_json::json;

    fn policy_fixture() -> super::super::policy::PolicyInfo {
        super::super::policy::parse_response(&json!({"data":{
            "timestamp":1, "runRuleModel":{"minDistance":1000}, "runAreaModels":[
                {"id":73,"name":"测试跑区","valid":true,"fenceIds":[701,702]},
                {"id":74,"name":"暂停跑区","valid":false,"fenceIds":[702]}
            ],
            "geoFence":{"geoFences":[
                {"id":701,"points":[{"lat":30.,"lon":110.},{"lat":30.01,"lon":110.},{"lat":30.,"lon":110.01}]},
                {"id":702,"points":[{"lat":31.,"lon":110.},{"lat":31.01,"lon":110.},{"lat":31.,"lon":110.01}]}
            ]}
        }})).unwrap()
    }

    #[test]
    fn policy_areas_map_multiple_fences_and_share_fences_without_using_fence_id_as_area() {
        let p = policy_fixture();
        let r = p.geo_fence.unwrap();
        let list = catalog_with_areas(&r, &p.run_areas).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].polygons.len(), 2);
        assert_eq!(list[1].polygons, vec![r.polygons[1].clone()]);
        assert_eq!(list[0].run_area_id(999).unwrap(), Some(73));
        let sel = Selection { school:"school".into(), key:list[0].key.clone(), confirmed:true };
        let picked = selected_response(r.clone(), &p.run_areas, Some(&sel), "school").unwrap();
        assert_eq!(picked.area.run_area_id, 73);
        assert_eq!(picked.area.geo_fences[0]["id"], 701);
        let invalid = Selection { key:list[1].key.clone(), ..sel.clone() };
        assert!(selected_response(r.clone(), &p.run_areas, Some(&invalid), "school").err().unwrap().contains("无效"));
        assert!(selected_response(r.clone(), &p.run_areas, Some(&sel), "other").is_err());
        let mut changed = p.run_areas;
        changed[0].id = 75;
        assert!(selected_response(r, &changed, Some(&sel), "school").is_err());
    }

    #[test]
    fn policy_area_relations_reject_missing_duplicate_and_conflicting_fences() {
        let p = policy_fixture();
        let r = p.geo_fence.unwrap();
        let mut missing = r.clone();
        missing.area.geo_fences.remove(0);
        missing.polygons.remove(0);
        assert!(catalog_with_areas(&missing, &p.run_areas).is_err());
        let mut duplicate = r.clone();
        duplicate.area.geo_fences.push(r.area.geo_fences[0].clone());
        duplicate.polygons.push(r.polygons[0].clone());
        assert!(catalog_with_areas(&duplicate, &p.run_areas).is_err());
        let mut conflicting = r;
        conflicting.area.geo_fences[0]["runAreaId"] = json!(999);
        assert!(catalog_with_areas(&conflicting, &p.run_areas).is_err());
    }

    #[test]
    fn malformed_policy_areas_do_not_become_legacy_unscoped_requests() {
        let valid = json!({"id":"73","name":"测试跑区","valid":true,"fenceIds":["701",701]});
        let parsed = parse_run_areas(Some(&json!([valid.clone()]))).unwrap();
        assert_eq!(parsed[0].fence_ids, vec![701]);
        assert!(parse_run_areas(Some(&json!([valid.clone(),valid.clone()]))).is_err());
        for field in ["id","valid","name","fenceIds"] {
            let mut v = valid.clone(); v.as_object_mut().unwrap().remove(field);
            assert!(parse_run_areas(Some(&json!([v]))).is_err(), "{field}");
        }
        for (field, value) in [("id",json!(-1)),("valid",json!("true")),("fenceIds",json!([])),("fenceIds",json!([null]))] {
            let mut v = valid.clone(); v[field] = value;
            assert!(parse_run_areas(Some(&json!([v]))).is_err());
        }
        assert!(parse_run_areas(Some(&json!({}))).is_err());
        assert!(parse_run_areas(None).unwrap().is_empty());
        assert!(parse_run_areas(Some(&json!(null))).unwrap().is_empty());
    }
    #[test]
    fn points_area_uses_explicit_selected_id_without_inventing_one() {
        let mut c = Campus {
            run_area: None,
            key: String::new(),
            name: String::new(),
            polygons: vec![],
            fences: vec![serde_json::json!({"id":999})],
        };
        assert_eq!(c.run_area_id(-1).unwrap(), None);
        assert_eq!(c.run_area_id(42).unwrap(), Some(42));
        c.fences = vec![serde_json::json!({"runAreaId":"73"})];
        assert_eq!(c.run_area_id(42).unwrap(), Some(73));
        c.fences.push(serde_json::json!({"runAreaId":73}));
        assert_eq!(c.run_area_id(-1).unwrap(), Some(73));
        c.fences.push(serde_json::json!({"runAreaId":74}));
        assert!(c.run_area_id(42).is_err());
    }
}
