use super::{Clip, TileScores};
use crate::reason::{any_nan, first_hit, Reason};

pub fn judge(c: &Clip) -> Option<Reason> {
    if any_nan(&[c.manmade_p, c.close_p, c.window_p]) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::ManmadeScene, c.manmade_p >= 0.5),
        (Reason::GardenOrLookingDown, c.close_p >= 0.7),
        (Reason::ThroughWindow, c.window_p >= 0.6),
    ])
}

pub fn judge_tiles(t: &TileScores) -> Option<Reason> {
    if any_nan(&[t.toon, t.person]) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::ToonTile, t.toon >= 0.25),
        (Reason::PersonTile, t.person >= 0.6),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> Clip {
        Clip {
            close_p: 0.5,
            manmade_p: 0.0,
            window_p: 0.0,
        }
    }

    /// 場面のルール：CLIP で「街・建物・部屋・絵」などの文章に寄る割合が0.5以上なら弾くこと（試作は Places365 で、自然の場所らしさ0.6未満か屋外らしさ0.85未満。CLIP の方が湖や渓流を巻き込まず、電柱や歩道を弾けたので置き換えた）
    #[test]
    fn test_scene_rule() {
        let scene = |manmade_p: f64| {
            judge(&Clip {
                manmade_p,
                ..passing()
            })
        };
        assert_eq!(scene(0.5), Some(Reason::ManmadeScene));
        assert_eq!(scene(0.4999), None);
    }

    /// どのルールにも当てはまらない数値は通すこと
    #[test]
    fn test_passing_values_pass() {
        assert_eq!(judge(&passing()), None);
    }

    /// CLIP のルール：庭・見下ろしの割合0.7以上を弾くこと（庭・見下ろしは「少し遠景であること」の方針で、木の幹や茂みの大写しを落とすため試作由来の0.884から下げた）
    #[test]
    fn test_garden_rule() {
        let close = |close_p: f64| {
            judge(&Clip {
                close_p,
                ..passing()
            })
        };
        assert_eq!(close(0.7), Some(Reason::GardenOrLookingDown));
        assert_eq!(close(0.6999), None);
    }

    /// 窓ごしのルール：「車の中から撮った写真」側の割合が0.6以上なら弾くこと（車の窓ごしに見た巨人の絵 A_73365 0.88 や雨粒の窓ごしの木 A_13821 0.84 を落とすため。残す通過写真の最大は車窓の木々 A_66191 の0.30）
    #[test]
    fn test_window_rule() {
        let window = |window_p: f64| {
            judge(&Clip {
                window_p,
                ..passing()
            })
        };
        assert_eq!(window(0.6), Some(Reason::ThroughWindow));
        assert_eq!(window(0.5999), None);
    }

    /// 当てはまるルールが複数あるときは、人工物の場面→庭・見下ろし→窓ごしの順で理由を決めること
    #[test]
    fn test_reasons_in_order() {
        let all = Clip {
            close_p: 0.999,
            manmade_p: 0.9,
            window_p: 0.9,
        };
        assert_eq!(judge(&all), Some(Reason::ManmadeScene));
        assert_eq!(
            judge(&Clip {
                manmade_p: 0.0,
                ..all
            }),
            Some(Reason::GardenOrLookingDown)
        );
        assert_eq!(
            judge(&Clip {
                manmade_p: 0.0,
                close_p: 0.0,
                ..all
            }),
            Some(Reason::ThroughWindow)
        );
    }

    /// 人工物・庭・窓ごしの割合のどれか1つでも計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        for c in [
            Clip {
                manmade_p: f64::NAN,
                ..passing()
            },
            Clip {
                close_p: f64::NAN,
                ..passing()
            },
            Clip {
                window_p: f64::NAN,
                ..passing()
            },
        ] {
            assert_eq!(judge(&c), Some(Reason::Unmeasured), "{c:?}");
        }
    }

    /// 3×3 のマスのどれかでアニメ調の割合が0.25以上なら弾き、測れなければ「未測定」にすること（景色の中に小さく写ったアニメのぬいぐるみやグッズ、アニメ絵の川 E_73700・ゲーム CG の温泉 E_12526 を捕まえるため。マスにはアニメ調の文章に「ぬいぐるみ」「ゲームの場面・CG・アニメの場面」を足して聞く。良い写真も数枚落ちるが、線が引けないときは除外を優先する方針）
    #[test]
    fn test_tile_rule() {
        let toon = |toon: f64| judge_tiles(&TileScores { toon, person: 0.0 });
        assert_eq!(toon(0.25), Some(Reason::ToonTile));
        assert_eq!(toon(0.2499), None);
        assert_eq!(toon(f64::NAN), Some(Reason::Unmeasured));
    }

    /// 3×3 のマスのどれかで人の割合が0.6以上なら弾き、測れなければ「未測定」にすること（写真全体の物探しでは見間違いと区別できない人を拾うため。赤いマントの人 H_209952 0.71・尾根に立つ人 G_151817 0.73 は弾き、ぼけた落ち葉 H_207397 0.55 のような見間違いは通す）
    #[test]
    fn test_person_tile_rule() {
        let person = |person: f64| judge_tiles(&TileScores { toon: 0.0, person });
        assert_eq!(person(0.6), Some(Reason::PersonTile));
        assert_eq!(person(0.5999), None);
        assert_eq!(person(f64::NAN), Some(Reason::Unmeasured));
    }

    /// マスでアニメ調にも人にも当てはまるときは、アニメ調を理由にすること
    #[test]
    fn test_toon_tile_comes_before_person_tile() {
        assert_eq!(
            judge_tiles(&TileScores {
                toon: 0.9,
                person: 0.9
            }),
            Some(Reason::ToonTile)
        );
    }
}
