use std::path::{Path, PathBuf};

use crate::judge::{
    judge_faces, judge_foreground, judge_late, judge_light, judge_objects, judge_ocr, judge_scene,
    judge_tiles, Late, Reason,
};
use crate::stage1::{self, Stage1Verdict};
use crate::stage2::clip::{Clip, ClipScorer};
use crate::stage2::faces::FaceDetectors;
use crate::stage2::foreground::ForegroundDetector;
use crate::stage2::objects::ObjectDetector;
use crate::stage2::ocr::TextDetector;
use crate::stage2::{depth, measure_light, near};
use crate::too_elongated;
use vision::img::{self, Image};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Stage1(Stage1Verdict),
    Stage1Unmeasured,
    Stage2(Reason),
}

pub struct Models {
    dir: PathBuf,
}

impl Models {
    pub fn load(models_dir: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            return_large_blocks_directly(),
            "failed to set the malloc mmap threshold"
        );
        drop(FaceDetectors::load(models_dir)?);
        drop(ObjectDetector::load(models_dir)?);
        drop(TextDetector::load(models_dir)?);
        drop(ClipScorer::load(models_dir)?);
        drop(ForegroundDetector::load(models_dir)?);
        release_memory();
        Ok(Self {
            dir: models_dir.to_path_buf(),
        })
    }
}

pub fn stage1(smalls: &[Vec<u8>]) -> Result<(), Rejection> {
    for bytes in smalls {
        let (img, _) = decode(bytes).ok_or(Rejection::Stage1Unmeasured)?;
        let features = stage1::features(&img).map_err(|_| Rejection::Stage1Unmeasured)?;
        match stage1::judge(&features) {
            Stage1Verdict::Pass => {}
            verdict => return Err(Rejection::Stage1(verdict)),
        }
    }
    Ok(())
}

pub fn stage2(models: &Models, larges: &[Vec<u8>]) -> Result<(), Rejection> {
    for bytes in larges {
        drop(open_large(bytes)?);
    }
    let open = |i: usize| open_large(&larges[i]).ok();
    let clips = std::cell::RefCell::new(vec![None; larges.len()]);
    let verdict = judge_all(
        larges.len(),
        |i| {
            let Some((bgr, gray)) = open(i) else {
                return Some(Reason::Unmeasured);
            };
            judge_early_lazily(&models.dir, &bgr, &gray, &mut clips.borrow_mut()[i])
        },
        |i| {
            let (bgr, gray) = open(i)?;
            let clip = clips.borrow_mut()[i].take();
            measure_late(&models.dir, &bgr, &gray, clip)
        },
        |i| {
            let (bgr, _) = open(i)?;
            use_model(ClipScorer::load(&models.dir), |m| m.tile_toon_max(&bgr)).ok()
        },
        |i| {
            let (bgr, _) = open(i)?;
            use_model(ForegroundDetector::load(&models.dir), |m| m.measure(&bgr)).ok()
        },
    );
    release_memory();
    verdict
}

fn judge_all(
    count: usize,
    mut early: impl FnMut(usize) -> Option<Reason>,
    mut late: impl FnMut(usize) -> Option<Late>,
    mut tiles: impl FnMut(usize) -> Option<f64>,
    mut foreground: impl FnMut(usize) -> Option<f64>,
) -> Result<(), Rejection> {
    let unmeasured = Rejection::Stage2(Reason::Unmeasured);
    for i in 0..count {
        if let Some(reason) = early(i) {
            return Err(Rejection::Stage2(reason));
        }
    }
    for i in 0..count {
        if let Some(reason) = judge_late(&late(i).ok_or(unmeasured)?) {
            return Err(Rejection::Stage2(reason));
        }
    }
    for i in 0..count {
        if let Some(reason) = judge_tiles(tiles(i).ok_or(unmeasured)?) {
            return Err(Rejection::Stage2(reason));
        }
    }
    for i in 0..count {
        if let Some(reason) = judge_foreground(foreground(i).ok_or(unmeasured)?) {
            return Err(Rejection::Stage2(reason));
        }
    }
    Ok(())
}

fn open_large(bytes: &[u8]) -> Result<(Image, Image), Rejection> {
    let unmeasured = Rejection::Stage2(Reason::Unmeasured);
    let (bgr, gray) = decode(bytes).ok_or(unmeasured)?;
    if too_elongated(bgr.w as f64, bgr.h as f64) {
        return Err(Rejection::Stage2(Reason::Elongated));
    }
    Ok((bgr, gray))
}

fn judge_early_lazily(
    models: &Path,
    bgr: &Image,
    gray: &Image,
    clip: &mut Option<Clip>,
) -> Option<Reason> {
    let unmeasured = Some(Reason::Unmeasured);
    let Ok(light) = measure_light(bgr, gray) else {
        return unmeasured;
    };
    judge_light(&light, bgr.h.max(bgr.w) as i32)
        .or_else(
            || match use_model(ObjectDetector::load(models), |m| m.detect(bgr)) {
                Ok(objects) => judge_objects(&objects),
                Err(_) => unmeasured,
            },
        )
        .or_else(
            || match use_model(ClipScorer::load(models), |m| m.score(bgr)) {
                Ok(c) => {
                    let reason = judge_scene(c.manmade_p);
                    *clip = Some(c);
                    reason
                }
                Err(_) => unmeasured,
            },
        )
        .or_else(
            || match use_model(TextDetector::load(models), |m| m.detect(bgr)) {
                Ok(ocr) => judge_ocr(&ocr),
                Err(_) => unmeasured,
            },
        )
        .or_else(
            || match use_model(FaceDetectors::load(models), |m| m.measure(bgr)) {
                Ok(faces) => judge_faces(&faces),
                Err(_) => unmeasured,
            },
        )
}

fn measure_late(models: &Path, bgr: &Image, gray: &Image, clip: Option<Clip>) -> Option<Late> {
    let clip = match clip {
        Some(c) => c,
        None => use_model(ClipScorer::load(models), |m| m.score(bgr)).ok()?,
    };
    Some(Late {
        near: near::measure(bgr, gray).ok()?,
        sky: depth::measure(bgr).ok()?.sky,
        clip,
    })
}

fn use_model<M, T>(
    model: anyhow::Result<M>,
    run: impl FnOnce(&mut M) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let out = model.and_then(|mut m| run(&mut m));
    release_memory();
    out
}

const MMAP_THRESHOLD: i32 = 128 * 1024;

fn return_large_blocks_directly() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        return libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD) == 1;
    }
    #[allow(unreachable_code)]
    {
        let _ = MMAP_THRESHOLD;
        true
    }
}

fn release_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

fn decode(bytes: &[u8]) -> Option<(Image, Image)> {
    img::decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture_bytes, jpeg, models, solid};
    use crate::judge::{judge_early, Early};

    fn textured() -> Vec<u8> {
        fixture_bytes("textured_green.jpg")
    }

    /// 1段目は、全部の画像が通ったときだけ通すこと
    #[test]
    fn test_stage1_passes_only_when_all_images_pass() {
        assert_eq!(stage1(&[textured(), textured()]), Ok(()));
    }

    /// 1段目は、1枚でも弾かれたら投稿ごと弾き、その画像の理由を返すこと
    #[test]
    fn test_stage1_rejects_post_if_any_image_fails() {
        let flat = jpeg(&solid(128, 128, (60.0, 140.0, 60.0)));

        assert_eq!(
            stage1(&[textured(), flat]),
            Err(Rejection::Stage1(Stage1Verdict::Illustration))
        );
    }

    /// 1段目で画像として読めないデータは、止まらずに「測れない」として弾くこと
    #[test]
    fn test_stage1_rejects_undecodable_bytes() {
        assert_eq!(
            stage1(&[b"not an image".to_vec()]),
            Err(Rejection::Stage1Unmeasured)
        );
    }

    /// 大きいサムネイルの縦横比が3倍以上なら、重い計算の前に「細長い」として弾くこと（投稿データに縦横比が書かれていない場合のため）
    #[test]
    fn test_large_image_too_elongated_is_rejected_before_measuring() {
        let wide = jpeg(&solid(900, 300, (60.0, 140.0, 60.0)));

        assert_eq!(
            open_large(&wide).err(),
            Some(Rejection::Stage2(Reason::Elongated))
        );
        assert!(open_large(&jpeg(&solid(899, 300, (60.0, 140.0, 60.0)))).is_ok());
    }

    /// 2段目で画像として読めないデータは、止まらずに「未測定」として弾くこと
    #[test]
    fn test_large_undecodable_bytes_are_unmeasured() {
        assert_eq!(
            open_large(b"not an image").err(),
            Some(Rejection::Stage2(Reason::Unmeasured))
        );
    }

    /// Linux では、128KB 以上のメモリを共有の置き場（ヒープ）を使わず OS から直接借りる設定にできること（CLIP や文字探しの作業メモリでヒープが広がったまま戻らず、投稿の合間も約86MB 残っていたのを約27MB にするため）
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn test_large_blocks_are_returned_directly() {
        assert!(return_large_blocks_directly());
    }

    /// モデルが1つでも読み込めないときは、判定の準備自体を失敗にすること（一部のルールを外したまま動かさないため）
    #[test]
    fn test_models_fail_to_load_without_clip() {
        let dir = std::env::temp_dir().join(format!("silentviews-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(models()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_owned();
            if !name.to_string_lossy().starts_with("clip_") {
                std::os::unix::fs::symlink(&path, dir.join(name)).unwrap();
            }
        }

        let loaded = Models::load(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(loaded.is_err());
        assert!(Models::load(models()).is_ok());
    }

    fn early_with(change: impl Fn(&mut Early)) -> Option<Reason> {
        let mut e = crate::judge::tests::passing();
        change(&mut e);
        judge_early(&e)
    }

    fn late_with(change: impl Fn(&mut Late)) -> Late {
        let mut l = crate::judge::tests::passing_late();
        change(&mut l);
        l
    }

    /// 2段目は、全部の画像が前半も後半も通ったときだけ通すこと
    #[test]
    fn test_judge_all_passes_when_every_image_passes() {
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |_| Some(0.0),
            |_| Some(0.0),
        );

        assert_eq!(result, Ok(()));
    }

    /// 前半で1枚でも弾かれたら、後半を測らずに投稿ごと弾くこと（重い計算を省くため）
    #[test]
    fn test_judge_all_stops_before_late_when_early_rejects() {
        let mut late_calls = 0;
        let result = judge_all(
            2,
            |i| {
                early_with(|e| {
                    if i == 1 {
                        e.faces.yn_count = 2
                    }
                })
            },
            |_| {
                late_calls += 1;
                Some(late_with(|_| {}))
            },
            |_| Some(0.0),
            |_| Some(0.0),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::FaceYunet)));
        assert_eq!(late_calls, 0);
    }

    /// 前半を全部通っても、後半で1枚でも弾かれたら投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_when_late_rejects_any_image() {
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |i| {
                Some(late_with(|l| {
                    if i == 1 {
                        l.clip.toon_p = 0.9
                    }
                }))
            },
            |_| Some(0.0),
            |_| Some(0.0),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::Toon)));
    }

    /// 前半・後半・マスのどこかで測れなかった画像があれば、「未測定」として投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_unmeasured() {
        let early_fail = judge_all(
            2,
            |i| {
                if i == 0 {
                    early_with(|_| {})
                } else {
                    Some(Reason::Unmeasured)
                }
            },
            |_| Some(late_with(|_| {})),
            |_| Some(0.0),
            |_| Some(0.0),
        );
        let late_fail = judge_all(
            2,
            |_| early_with(|_| {}),
            |i| (i == 0).then(|| late_with(|_| {})),
            |_| Some(0.0),
            |_| Some(0.0),
        );
        let tiles_fail = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |i| (i == 0).then_some(0.0),
            |_| Some(0.0),
        );

        assert_eq!(early_fail, Err(Rejection::Stage2(Reason::Unmeasured)));
        assert_eq!(late_fail, Err(Rejection::Stage2(Reason::Unmeasured)));
        assert_eq!(tiles_fail, Err(Rejection::Stage2(Reason::Unmeasured)));
    }

    /// 後半で1枚でも弾かれたら、マスの判定をせずに投稿ごと弾くこと（マスの判定は CLIP を9回動かして重いため、最後に回す）
    #[test]
    fn test_judge_all_stops_before_tiles_when_late_rejects() {
        let mut tile_calls = 0;
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |i| {
                Some(late_with(|l| {
                    if i == 1 {
                        l.clip.toon_p = 0.9
                    }
                }))
            },
            |_| {
                tile_calls += 1;
                Some(0.0)
            },
            |_| Some(0.0),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::Toon)));
        assert_eq!(tile_calls, 0);
    }

    /// ほかを全部通っても、マスの判定で1枚でも弾かれたら投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_when_tiles_reject_any_image() {
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |i| Some(if i == 1 { 0.9 } else { 0.0 }),
            |_| Some(0.0),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::ToonTile)));
    }

    /// マスの判定で1枚でも弾かれたら、手前の物の判定をせずに投稿ごと弾くこと（手前の物の判定は目立つ物を切り出すモデルを動かして重いため、最後に回す）
    #[test]
    fn test_judge_all_stops_before_foreground_when_tiles_reject() {
        let mut foreground_calls = 0;
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |i| Some(if i == 1 { 0.9 } else { 0.0 }),
            |_| {
                foreground_calls += 1;
                Some(0.0)
            },
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::ToonTile)));
        assert_eq!(foreground_calls, 0);
    }

    /// ほかを全部通っても、手前の物の判定で1枚でも弾かれたら投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_when_foreground_rejects_any_image() {
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |_| Some(0.0),
            |i| Some(if i == 1 { 0.1 } else { 0.0 }),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::ForegroundObject)));
    }

    /// 手前の物を測れなかった画像があれば、「未測定」として投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_unmeasured_foreground() {
        let result = judge_all(
            2,
            |_| early_with(|_| {}),
            |_| Some(late_with(|_| {})),
            |_| Some(0.0),
            |i| (i == 0).then_some(0.0),
        );

        assert_eq!(result, Err(Rejection::Stage2(Reason::Unmeasured)));
    }

    /// 人の写った写真は、後半に進まず2段目の前半で弾くこと（この写真は、上半身より先に「主役あり」で当てはまる）
    #[test]
    fn test_stage2_rejects_person_photo() {
        let m = Models::load(models()).unwrap();
        let photo = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/walking_person.jpg"
        ))
        .unwrap();

        assert_eq!(
            stage2(&m, &[photo]),
            Err(Rejection::Stage2(Reason::CenterSubject))
        );
    }
}
