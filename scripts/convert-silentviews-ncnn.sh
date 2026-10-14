#!/bin/sh
set -eu

dir="$(cd "$(dirname "$0")/.." && pwd)/crates/silentviews/consumer/models"
target=${1:-clip}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

tag=20260526
case "$(uname -s)" in
  Darwin)
    pnnx=pnnx-$tag-macos
    pnnx_sha=49f942bba47e0a6bbb971506f7beec73c12a7e5b74c7a90522eced006ca299d0
    ;;
  Linux)
    pnnx=pnnx-$tag-linux
    pnnx_sha=dde08ce2e2e1744a4ddce576910b451a860663af91d2aeed0d21ebee4fa69955
    ;;
  *)
    echo "unsupported OS" >&2
    exit 1
    ;;
esac

sha() {
  shasum -a 256 "$1" | cut -d' ' -f1
}

fetch() {
  out=$1
  url=$2
  expected=$3
  curl -fsSL -o "$out" "$url"
  actual=$(sha "$out")
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch: $url ($actual)" >&2
    exit 1
  fi
}

fetch "$work/pnnx.zip" "https://github.com/pnnx/pnnx/releases/download/$tag/$pnnx.zip" "$pnnx_sha"
(cd "$work" && unzip -q pnnx.zip)

convert() {
  stem=$1
  shape=$2
  param_sha=$3
  bin_sha=$4
  (cd "$work" && "./$pnnx/pnnx" "$stem.onnx" "inputshape=$shape" fp16=0 > "$stem.log" 2>&1)
  if [ "$stem" = face_detection_yunet_2023mar ]; then
    sed -i.orig -E '/^Reshape /s/ 1=(6400|1600|400)$/ 1=-1/' "$work/$stem.ncnn.param"
  fi
  for ext in param bin; do
    expected=$param_sha
    [ "$ext" = bin ] && expected=$bin_sha
    actual=$(sha "$work/$stem.ncnn.$ext")
    if [ "$actual" != "$expected" ]; then
      echo "unexpected $stem.ncnn.$ext ($actual)" >&2
      exit 1
    fi
    mv "$work/$stem.ncnn.$ext" "$dir/$stem.ncnn.$ext"
  done
  echo "converted $stem"
}

case "$target" in
  clip)
    cp "$dir/clip_vision.onnx" "$work/clip_vision.onnx"
    convert clip_vision '[1,3,224,224]' \
      3610e31ac5524ee7826210baeb89bde5ec46a73e3554ce58aa210d180f625d48 \
      a2940e9d7d77e4a1528d5a9d53ce06589b0a3c7fb5e6a39c9b1f3b3273d797e5
    ;;
  small)
    if [ "$(uname -s)" != Linux ]; then
      echo "small models must be converted on Linux (pnnx on macOS writes different weights)" >&2
      exit 1
    fi
    zoo=https://huggingface.co/opencv
    fetch "$work/text_detection_ppocrv3_2023may.onnx" \
      "$zoo/text_detection_ppocr/resolve/c299e81aedeaf5d4f741fccccea491fb32db005f/text_detection_en_ppocrv3_2023may.onnx" \
      03f550c6b406fda8bf54bd8327815f6c7e2edd98cea02348c93d879254366587
    fetch "$work/object_detection_nanodet_2022nov.onnx" \
      "$zoo/object_detection_nanodet/resolve/5bfd47077350a726ad440dd7bd1e1e35e8ebcfb2/object_detection_nanodet_2022nov.onnx" \
      4b82da9944b88577175ee23a459dce2e26e6e4be573def65b1055dc2d9720186
    fetch "$work/face_detection_yunet_2023mar.onnx" \
      "$zoo/face_detection_yunet/resolve/3cc26e7f1014a5ee5d74a42acee58bafc9d0a310/face_detection_yunet_2023mar.onnx" \
      8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4
    convert text_detection_ppocrv3_2023may '[1,3,736,736]' \
      c2a7ef325df25061668bae79a0431a4b4d10116c116e7de98dea52f821a8d016 \
      fefa67edb4af3b7fbff0802b9250956d012e77534de60183950e83c5b9a62649
    convert object_detection_nanodet_2022nov '[1,3,416,416]' \
      554ea94e635bee53e09216d842b536cba52c7cf8ab59e5f3bd5ef7d011191f47 \
      8f0f1d5cebb8721ac3544dd9edd29e8f2c21dd4df17bfb993772b1d54674e6a8
    convert face_detection_yunet_2023mar '[1,3,640,640]' \
      8d4c649e606d816a2ec69851070bbaf6d8abfd1e983e48a779fc84c9fcf1d014 \
      8faa696d61ad6bf5c13ed5d6c8ae35e376c660968db53d723a329020856ca5c9
    ;;
  *)
    echo "usage: convert-silentviews-ncnn.sh [clip|small]" >&2
    exit 1
    ;;
esac
