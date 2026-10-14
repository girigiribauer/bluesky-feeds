use std::collections::BTreeMap;

use serde_json::Value;
use silentviews_consumer::judge::{judge_early, judge_late, Early, Late, Reason};
use silentviews_consumer::stage2::{
    clip::Clip,
    faces::Faces,
    near::Near,
    objects::{Object, COCO},
    ocr::Ocr,
    LightFeatures,
};

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64()
}

fn build(v: &Value) -> Option<(Early, Option<Late>)> {
    let g = |k: &str| num(v, k);
    let light = LightFeatures {
        blur: g("blur")?,
        flat: g("flat")?,
        text_lines: g("text_lines").unwrap_or(0.0) as u32,
        text_area: g("text_area")?,
        horiz: g("horiz")? as u32,
        vert: g("vert")? as u32,
        diag: g("diag")? as u32,
        edge: g("edge")?,
        green: g("green")?,
        blue: g("blue")?,
        earth: g("earth")?,
        sat_mean: g("sat_mean")?,
        sat_hi: g("sat_hi")?,
        sky: g("sky")?,
        near_blob: g("near_blob")?,
        center_near: g("center_near")?,
        side_near: g("side_near")?,
        smooth_share: g("smooth_share")?,
        dead_flat: g("dead_flat")?,
        dead_flat_ns: g("dead_flat_ns")?,
        ground_gray: g("ground_gray")?,
        mirror: g("mirror")?,
        aspect: g("aspect")?,
        seg_hv: g("seg_hv")? as u32,
        seg_v: g("seg_v")? as u32,
        corners: g("corners")? as u32,
        vivid_hues: g("vivid_hues")? as u32,
        colorfulness: g("colorfulness")?,
    };
    let objects = v
        .get("objects")?
        .as_array()?
        .iter()
        .map(|o| {
            let name = o[0].as_str().unwrap();
            Object {
                name: COCO.iter().find(|c| **c == name).unwrap(),
                conf: o[1].as_f64().unwrap(),
                area: o[2].as_f64().unwrap(),
            }
        })
        .collect();
    let early = Early {
        light,
        faces: Faces {
            yn_count: g("yn_count")? as u32,
            yn_max_area: g("yn_max_area")?,
        },
        objects,
        ocr: Ocr {
            ocr_n: g("ocr_n")? as u32,
            ocr_area: g("ocr_area").unwrap_or(0.0),
            ocr_lines: g("ocr_lines")? as u32,
            ocr_edge: g("ocr_edge").unwrap_or(0.0) as u32,
        },
        max_side: g("max_side")? as i32,
        manmade_p: g("manmade_p")?,
    };
    let late = (|| {
        Some(Late {
            near: Near {
                rt_min: g("rt_min")?,
                ln_count: g("ln_count"),
                top_smooth: g("top_smooth")?,
            },
            clip: Clip {
                toon_p: g("toon_p")?,
                close_p: g("close_p")?,
                manmade_p: g("manmade_p")?,
                window_p: g("window_p")?,
            },
            sky: g("sky")?,
        })
    })();
    Some((early, late))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let feats: BTreeMap<String, Value> = serde_json::from_str(&std::fs::read_to_string(&args[1])?)?;
    let mut out = BTreeMap::new();
    for (k, v) in &feats {
        let verdict = match build(v) {
            None => Some(Reason::Unmeasured),
            Some((e, late)) => judge_early(&e).or_else(|| match late {
                Some(l) => judge_late(&l),
                None => Some(Reason::Unmeasured),
            }),
        };
        out.insert(k.clone(), format!("{:?}", verdict));
    }
    std::fs::write(&args[2], serde_json::to_string(&out)?)?;
    Ok(())
}
