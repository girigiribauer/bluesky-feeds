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
  shape2=${5:-}
  (cd "$work" && "./$pnnx/pnnx" "$stem.onnx" "inputshape=$shape" ${shape2:+"inputshape2=$shape2"} fp16=0 > "$stem.log" 2>&1)
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
    fetch "$work/text_recognition_ppocrv4.onnx" \
      "https://huggingface.co/SWHL/RapidOCR/resolve/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx" \
      48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b
    fetch "$work/u2netp.onnx" \
      "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx" \
      309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8
    convert text_detection_ppocrv3_2023may '[1,3,736,736]' \
      c2a7ef325df25061668bae79a0431a4b4d10116c116e7de98dea52f821a8d016 \
      fefa67edb4af3b7fbff0802b9250956d012e77534de60183950e83c5b9a62649
    convert object_detection_nanodet_2022nov '[1,3,416,416]' \
      554ea94e635bee53e09216d842b536cba52c7cf8ab59e5f3bd5ef7d011191f47 \
      8f0f1d5cebb8721ac3544dd9edd29e8f2c21dd4df17bfb993772b1d54674e6a8
    convert text_recognition_ppocrv4 '[1,3,48,320]' \
      2ae30dfdc7ee482c6cbbc0419c16ee125e880e9eaf69053d61953694bb0fcf4c \
      f91ebdcf4d6b58c58a87704160e59d07ae56bc2d5021eae09d095c5a20bf7525 \
      '[1,3,48,640]'
    convert u2netp '[1,3,320,320]' \
      faf6e6aa63b8934e7077f4100deeac5f590f64931da03b0000133e5895d1dc42 \
      7e18d54080bc4560dff8f9b39f4a5b0392f6a104cf4d59b4c5d86bd24402c908
    ;;
  *)
    echo "usage: convert-silentviews-ncnn.sh [clip|small]" >&2
    exit 1
    ;;
esac
