use std::path::{Path, PathBuf};

use crate::reason::Reason;
use crate::too_elongated;
use crate::tools::clip_words::{self, ClipScorer};
use crate::tools::foreground_objects::{self, ForegroundDetector};
use crate::tools::object_detector::{self, ObjectDetector};
use crate::tools::text_detector::{self, TextDetector};
use crate::tools::text_reader::TextReader;
use crate::tools::thumb_colors;
use crate::tools::{edge_amount, flat_areas, mirror_symmetry, nearness, scene_zones, text_texture};
use vision::img::{self, Image};

pub struct Models {
    dir: PathBuf,
}

impl Models {
    pub fn load(models_dir: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            return_large_blocks_directly(),
            "failed to set the malloc mmap threshold"
        );
        drop(ObjectDetector::load(models_dir)?);
        drop(TextDetector::load(models_dir)?);
        drop(TextReader::load(models_dir)?);
        drop(ClipScorer::load(models_dir)?);
        drop(ForegroundDetector::load(models_dir)?);
        release_memory();
        Ok(Self {
            dir: models_dir.to_path_buf(),
        })
    }
}

pub fn judge_thumbnails(thumbnails: &[Vec<u8>]) -> Result<(), Reason> {
    for bytes in thumbnails {
        let (thumb, _) = img::decode(bytes).ok_or(Reason::Unmeasured)?;
        check(thumb_colors::judge(&measured(thumb_colors::measure(
            &thumb,
        ))?))?;
    }
    Ok(())
}

pub fn judge_images(models: &Models, larges: &[Vec<u8>]) -> Result<(), Reason> {
    for bytes in larges {
        drop(open_large(bytes)?);
    }
    let dir = &models.dir;
    let open = |i: usize| open_large(&larges[i]);
    let verdict = judge_all(
        larges.len(),
        &mut [
            &mut |i| {
                let (bgr, gray) = open(i)?;
                judge_basic(dir, &bgr, &gray)
            },
            &mut |i| {
                let (bgr, _) = open(i)?;
                let scores = use_model(ClipScorer::load(dir), |m| m.tile_scores(&bgr));
                check(clip_words::judge_tiles(&measured(scores)?))
            },
            &mut |i| {
                let (bgr, _) = open(i)?;
                let share = use_model(ForegroundDetector::load(dir), |m| m.measure(&bgr));
                check(foreground_objects::judge(measured(share)?))
            },
        ],
    );
    release_memory();
    verdict
}

type Phase<'a> = &'a mut dyn FnMut(usize) -> Result<(), Reason>;

fn judge_all(count: usize, phases: &mut [Phase]) -> Result<(), Reason> {
    for phase in phases.iter_mut() {
        for i in 0..count {
            phase(i)?;
        }
    }
    Ok(())
}

fn check(reason: Option<Reason>) -> Result<(), Reason> {
    reason.map_or(Ok(()), Err)
}

fn measured<T>(value: anyhow::Result<T>) -> Result<T, Reason> {
    value.map_err(|_| Reason::Unmeasured)
}

fn open_large(bytes: &[u8]) -> Result<(Image, Image), Reason> {
    let (bgr, gray) = img::decode(bytes).ok_or(Reason::Unmeasured)?;
    if too_elongated(bgr.w as f64, bgr.h as f64) {
        return Err(Reason::Elongated);
    }
    Ok((bgr, gray))
}

fn judge_basic(models: &Path, bgr: &Image, gray: &Image) -> Result<(), Reason> {
    if bgr.ch != 3 || bgr.w == 0 || bgr.h == 0 || gray.ch != 1 || gray.w == 0 || gray.h == 0 {
        return Err(Reason::Unmeasured);
    }
    let flat = flat_areas::measure(bgr, gray);
    check(flat_areas::judge(&flat))?;
    check(text_texture::judge(text_texture::measure(bgr)))?;
    let zones = measured(scene_zones::measure(bgr))?;
    check(scene_zones::judge(&zones, flat.smooth_share))?;
    check(mirror_symmetry::judge(mirror_symmetry::measure(bgr)))?;
    check(edge_amount::judge(edge_amount::measure(bgr), zones.sky))?;
    let objects = measured(use_model(ObjectDetector::load(models), |m| m.detect(bgr)))?;
    check(object_detector::judge(&objects))?;
    let clip = measured(use_model(ClipScorer::load(models), |m| m.score(bgr)))?;
    check(clip_words::judge(&clip))?;
    let boxes = measured(use_model(TextDetector::load(models), |m| m.polygons(bgr)))?;
    let boxes = if boxes.is_empty() {
        boxes
    } else {
        measured(use_model(TextReader::load(models), |m| {
            m.text_like(bgr, gray, boxes)
        }))?
    };
    check(text_detector::judge(&text_detector::summarize(
        &boxes, bgr.w, bgr.h,
    )))?;
    let near = measured(nearness::measure(bgr, gray))?;
    check(nearness::judge(&near, zones.sky))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture_bytes, jpeg, models, solid};

    fn textured() -> Vec<u8> {
        fixture_bytes("textured_green.jpg")
    }

    /// サムネイルの判定は、全部のサムネイルが通ったときだけ通すこと
    #[test]
    fn test_thumbnails_passes_only_when_all_images_pass() {
        assert_eq!(judge_thumbnails(&[textured(), textured()]), Ok(()));
    }

    /// サムネイルの判定は、1枚でも弾かれたら投稿ごと弾き、その画像の理由を返すこと
    #[test]
    fn test_thumbnails_rejects_post_if_any_image_fails() {
        let flat = jpeg(&solid(128, 128, (60.0, 140.0, 60.0)));

        assert_eq!(
            judge_thumbnails(&[textured(), flat]),
            Err(Reason::ThumbIllustration)
        );
    }

    /// サムネイルが画像として読めないデータなら、止まらずに「未測定」として弾くこと
    #[test]
    fn test_thumbnails_rejects_undecodable_bytes() {
        assert_eq!(
            judge_thumbnails(&[b"not an image".to_vec()]),
            Err(Reason::Unmeasured)
        );
    }

    /// 大きいサムネイルの縦横比が3倍以上なら、重い計算の前に「細長い」として弾くこと（投稿データに縦横比が書かれていない場合のため）
    #[test]
    fn test_large_image_too_elongated_is_rejected_before_measuring() {
        let wide = jpeg(&solid(900, 300, (60.0, 140.0, 60.0)));

        assert_eq!(open_large(&wide).err(), Some(Reason::Elongated));
        assert!(open_large(&jpeg(&solid(899, 300, (60.0, 140.0, 60.0)))).is_ok());
    }

    /// 大きい画像が画像として読めないデータなら、止まらずに「未測定」として弾くこと
    #[test]
    fn test_large_undecodable_bytes_are_unmeasured() {
        assert_eq!(open_large(b"not an image").err(), Some(Reason::Unmeasured));
    }

    /// 空の画像は、どの道具でも測らずに「未測定」として弾くこと（判定に失敗した画像は載せないため）
    #[test]
    fn test_basic_rejects_empty_image_as_unmeasured() {
        let result = judge_basic(models(), &Image::new(0, 0, 3), &Image::new(0, 0, 1));

        assert_eq!(result, Err(Reason::Unmeasured));
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

    /// どの段階でも全部の画像が通ったときだけ、投稿を通すこと
    #[test]
    fn test_judge_all_passes_when_every_image_passes_every_phase() {
        let result = judge_all(2, &mut [&mut |_| Ok(()), &mut |_| Ok(())]);

        assert_eq!(result, Ok(()));
    }

    /// ある段階で1枚でも弾かれたら、その理由で投稿ごと弾き、後の段階は1枚も測らないこと（重い計算を後ろに置いて省くため）
    #[test]
    fn test_judge_all_stops_before_later_phases_when_any_image_is_rejected() {
        let mut later_calls = 0;
        let result = judge_all(
            2,
            &mut [
                &mut |i| {
                    if i == 1 {
                        Err(Reason::ToonTile)
                    } else {
                        Ok(())
                    }
                },
                &mut |_| {
                    later_calls += 1;
                    Ok(())
                },
            ],
        );

        assert_eq!(result, Err(Reason::ToonTile));
        assert_eq!(later_calls, 0);
    }

    /// ある段階で測れなかった画像があれば、「未測定」として投稿ごと弾くこと
    #[test]
    fn test_judge_all_rejects_unmeasured() {
        let result = judge_all(
            2,
            &mut [&mut |i| check((i == 1).then_some(Reason::Unmeasured))],
        );

        assert_eq!(result, Err(Reason::Unmeasured));
    }

    /// 真ん中を人が歩いている写真は、重いマスの判定に進む前に「主役あり」で弾くこと
    #[test]
    fn test_images_rejects_person_photo() {
        let m = Models::load(models()).unwrap();
        let photo = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/walking_person.jpg"
        ))
        .unwrap();

        assert_eq!(judge_images(&m, &[photo]), Err(Reason::CenterSubject));
    }
}
