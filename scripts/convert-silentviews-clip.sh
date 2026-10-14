#!/bin/sh
set -eu

dir="$(cd "$(dirname "$0")/.." && pwd)/crates/silentviews/consumer/models"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

uv venv -q "$work/venv"
uv pip install -q -p "$work/venv/bin/python" torch==2.14.1
uv pip install -q -p "$work/venv/bin/python" transformers==5.20.0 safetensors onnx

mkdir "$work/model"
cp "$dir/tinyclip8m.safetensors" "$work/model/model.safetensors"
cp "$dir/tinyclip8m_config.json" "$work/model/config.json"
cp "$dir/tinyclip8m_tokenizer.json" "$work/model/tokenizer.json"
cp "$dir/tinyclip8m_preprocessor_config.json" "$work/model/preprocessor_config.json"

"$work/venv/bin/python" - "$work/model" "$dir" <<'PY'
import json
import sys

import torch
from transformers import CLIPModel, CLIPTokenizerFast

PHOTO = ["a photo of nature", "a photo of plants", "a photo of a flower", "a photo of grass", "a photo of an animal",
         "a photo of a landscape", "a photo of rocks", "a photo of trees", "a photo of water", "a photo of the sky"]
FAR = ["a landscape photo", "a scenic view with distant mountains", "a wide view of a forest", "a view of the sea and the horizon",
       "a photo of the sky and clouds", "an aerial photo", "a wide shot of a field", "a photo of trees in a park"]
NEAR = ["a close-up photo", "a macro photo", "a photo looking down at the ground", "a close-up photo of flowers",
        "a close-up photo of mushrooms", "a close-up photo of leaves", "a photo of a garden bed"]
TOON = ["an anime character", "a cartoon character", "a 3D video game character", "an illustration", "a screenshot of a video game"]
WINDOW = ["a photo taken from inside a car", "a view through a car windshield"]
TILE_TOON = TOON + ["a plush toy", "a stuffed toy", "a video game scene", "a 3D render", "an anime scene", "computer graphics"]
SCENE_NATURE = ["a photo of nature", "a photo of a landscape", "a photo of a forest", "a photo of mountains",
                "a photo of the sea", "a photo of the sky", "a photo of a field", "a photo of a lake",
                "a photo of a river", "a photo of trees", "a photo of flowers", "a photo of the countryside",
                "a photo of clouds", "a photo of a sunset"]
SCENE_MANMADE = ["a photo of a city", "a photo of a building", "a photo of a street", "a photo of a house",
                 "a photo of a room", "an indoor photo", "a photo of a parking lot", "a photo of a road",
                 "a photo of a shop", "a photo of a construction site", "a photo of a playground",
                 "a photo of a backyard", "a photo of a car", "a photo of a boat", "a photo of a bridge",
                 "a photo of power lines", "a photo of a tent", "a photo of a village", "a painting",
                 "a digital illustration", "a fantasy artwork", "a drawing", "an AI generated image"]

src, out = sys.argv[1], sys.argv[2]
m = CLIPModel.from_pretrained(src).eval()
tok = CLIPTokenizerFast.from_pretrained(src)
pre = json.load(open(f"{src}/preprocessor_config.json"))


def embed(prompts):
    with torch.no_grad():
        t = m.text_projection(m.text_model(**tok(prompts, padding=True, return_tensors="pt")).pooler_output)
    return (t / t.norm(dim=-1, keepdim=True)).numpy().tolist()


a, b = embed(PHOTO + TOON), embed(FAR + NEAR)
c = embed(SCENE_NATURE + SCENE_MANMADE)
d = embed(WINDOW)
e = embed(TILE_TOON)
json.dump({"photo": a[:len(PHOTO)], "toon": a[len(PHOTO):], "far": b[:len(FAR)], "near": b[len(FAR):],
           "scene_nature": c[:len(SCENE_NATURE)], "scene_manmade": c[len(SCENE_NATURE):], "window": d, "tile_toon": e},
          open(f"{out}/clip_text_embeds.json", "w"))
size = m.config.vision_config.image_size
json.dump({"image_size": size, "mean": pre["image_mean"], "std": pre["image_std"], "logit_scale": float(m.logit_scale.exp())},
          open(f"{out}/clip_meta.json", "w"))


class Vision(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, pixel_values):
        return self.m.visual_projection(self.m.vision_model(pixel_values=pixel_values).pooler_output)


torch.onnx.export(Vision(m), (torch.zeros(1, 3, size, size),), f"{out}/clip_vision.onnx",
                  input_names=["pixel_values"], output_names=["image_embeds"], opset_version=17, dynamo=False)
PY
echo "converted clip_vision.onnx, clip_text_embeds.json, clip_meta.json"
"$(dirname "$0")/convert-silentviews-ncnn.sh" clip
