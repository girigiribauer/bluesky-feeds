#!/bin/sh
set -eu

dir="$(cd "$(dirname "$0")/.." && pwd)/crates/silentviews/consumer/models"

fetch() {
  name=$1
  url=$2
  sha=$3
  if [ -f "$dir/$name" ] && [ "$(shasum -a 256 "$dir/$name" | cut -d' ' -f1)" = "$sha" ]; then
    return
  fi
  curl -fsSL -o "$dir/$name.part" "$url"
  actual=$(shasum -a 256 "$dir/$name.part" | cut -d' ' -f1)
  if [ "$actual" != "$sha" ]; then
    rm -f "$dir/$name.part"
    echo "checksum mismatch: $name ($actual)" >&2
    exit 1
  fi
  mv "$dir/$name.part" "$dir/$name"
  echo "fetched $name"
}

tinyclip=https://huggingface.co/wkcn/TinyCLIP-ViT-8M-16-Text-3M-YFCC15M/resolve/a2a8c6eaa2549ad66eb7c31b85022bf58273a26c

fetch tinyclip8m.safetensors \
  "$tinyclip/model.safetensors" \
  9339ee3d736344d0ddcaa6c03edc9f89688f08caaea5401220885233da726fcc

fetch tinyclip8m_config.json \
  "$tinyclip/config.json" \
  c586d856dbd5589b67525db9e791ee0917402405672c7773aead373efb7d7bca

fetch tinyclip8m_tokenizer.json \
  "$tinyclip/tokenizer.json" \
  b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842

fetch tinyclip8m_preprocessor_config.json \
  "$tinyclip/preprocessor_config.json" \
  910e70b3956ac9879ebc90b22fb3bc8a75b6a0677814500101a4c072bd7857bd
