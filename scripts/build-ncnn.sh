#!/bin/sh
set -eu

prefix=${1:?usage: build-ncnn.sh <install-dir> [source-dir]}
tag=20260526
commit=e54f7b1f88434e1d844ea0551b880a1cfb079ce1

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [ $# -ge 2 ]; then
  src=$2
else
  src="$work/ncnn"
  git clone -q --depth 1 --branch "$tag" https://github.com/Tencent/ncnn.git "$src"
fi
actual=$(git -C "$src" rev-parse HEAD)
if [ "$actual" != "$commit" ]; then
  echo "unexpected ncnn commit: $actual" >&2
  exit 1
fi

arch=""
if [ "$(uname -s)" = "Darwin" ]; then
  arch="-DCMAKE_OSX_ARCHITECTURES=$(rustc -vV | sed -n 's/^host: \([^-]*\)-.*/\1/p')"
fi

cmake -S "$src" -B "$work/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" $arch \
  -DNCNN_SHARED_LIB=OFF -DNCNN_VULKAN=OFF -DNCNN_OPENMP=OFF -DNCNN_THREADS=OFF \
  -DNCNN_BUILD_TOOLS=OFF -DNCNN_BUILD_EXAMPLES=OFF -DNCNN_BUILD_BENCHMARK=OFF -DNCNN_BUILD_TESTS=OFF \
  -DNCNN_SIMPLEOCV=OFF -DNCNN_PYTHON=OFF -DNCNN_C_API=ON \
  -DNCNN_AVX=OFF -DNCNN_FMA=OFF -DNCNN_F16C=OFF -DNCNN_XOP=OFF -DNCNN_AVX2=OFF \
  -DNCNN_AVXVNNI=OFF -DNCNN_AVXVNNIINT8=OFF -DNCNN_AVXVNNIINT16=OFF -DNCNN_AVXNECONVERT=OFF \
  -DNCNN_AVX512=OFF -DNCNN_AVX512VNNI=OFF -DNCNN_AVX512BF16=OFF -DNCNN_AVX512FP16=OFF
cmake --build "$work/build" -j "${JOBS:-4}"
cmake --install "$work/build"
echo "$commit" > "$prefix/ncnn-commit.txt"
