#!/usr/bin/env bash
# golden テスト用サードパーティ資産（フォント・CSL スタイル・CSL ロケール）を vendor/ へ取得する。
#
# 方針:
# - サードパーティ資産はリポジトリにコミットしない（再配布を発生させず、OFL / CC BY-SA の
#   再配布義務をリポジトリに負わせない）。公式リポジトリのピン留めコミット / リリースから
#   取得し、SHA-256 で内容を検証する（golden テストの決定性はバイト同一性に依存する）。
# - フォント: SIL Open Font License 1.1（ライセンス全文も併せて取得し vendor/fonts/licenses/ に置く）
# - CSL スタイル・ロケール: CC BY-SA 3.0（ライセンス表記はファイル内ヘッダに含まれる）
# - 冪等: 既に存在しハッシュ一致するファイルはスキップする。ハッシュ不一致は上書きせず失敗させる
#   （上流改変・破損の検出。意図した更新はマニフェストのハッシュを書き換えてから再実行する）。
#
# 使い方: tools/fetch-test-assets.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# 取得元（すべてコミット / リリースタグでピン留め）
GF="https://raw.githubusercontent.com/google/fonts/6bc5aaa80150ffda7799ea091674125880945c0a/ofl"
STYLES="https://raw.githubusercontent.com/citation-style-language/styles/d506f4c7697f033776689c4b79b19afa86ba4934"
LOCALES="https://raw.githubusercontent.com/citation-style-language/locales/bc0a222b4ca526126faf892c35b2b7b1215c10eb"
SHCJ_REL="https://github.com/adobe-fonts/source-han-code-jp/releases/download/2.012R"
SHCJ_RAW="https://raw.githubusercontent.com/adobe-fonts/source-han-code-jp/2.012R"

# マニフェスト: <vendor/ 相対パス>|<SHA-256>|<URL>（URL 中の [ ] は %5B %5D にエンコード済み）
MANIFEST="\
fonts/NotoSans[wdth,wght].ttf|bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d|$GF/notosans/NotoSans%5Bwdth,wght%5D.ttf
fonts/NotoSans-Italic[wdth,wght].ttf|58e6e0ebd1931b29a365aa2d3e2ee9a9e831a3af7cf3ad1462d4e72154f0b291|$GF/notosans/NotoSans-Italic%5Bwdth,wght%5D.ttf
fonts/NotoSansJP[wght].ttf|c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f|$GF/notosansjp/NotoSansJP%5Bwght%5D.ttf
fonts/NotoSerifJP[wght].ttf|2fd527ba12b6a44ec30d796d633360da0aeba6c5d4af1304ce12bb4dc15a7dfc|$GF/notoserifjp/NotoSerifJP%5Bwght%5D.ttf
fonts/JetBrainsMono[wght].ttf|48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda|$GF/jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf
fonts/JetBrainsMono-Italic[wght].ttf|85ae2a5cd3f56baf1ce1c21a851322c58e3d8fbe8e8ad4a4d090a820dd7fe558|$GF/jetbrainsmono/JetBrainsMono-Italic%5Bwght%5D.ttf
fonts/STIXTwoMath-Regular.ttf|562551b15b836e6e01d1b7350909baf3c8c8d83260c1190fbf4544333e6936de|$GF/stixtwomath/STIXTwoMath-Regular.ttf
fonts/STIXTwoText[wght].ttf|7962b8b7811e6a896c9a91a0bccbb5241047770eb24d4997c5cb5fe21d5c0df2|$GF/stixtwotext/STIXTwoText%5Bwght%5D.ttf
fonts/STIXTwoText-Italic[wght].ttf|88c0e2e316eaff56eddc9e51e4850317e2a1e490bbf758b2dec4793aedba9c74|$GF/stixtwotext/STIXTwoText-Italic%5Bwght%5D.ttf
fonts/SourceHanCodeJP.ttc|f430938b1056e79ff60cf422876e3bbbd348f578d38ff643074e2d6992cd95f5|$SHCJ_REL/SourceHanCodeJP.ttc
fonts/licenses/NotoSans.OFL.txt|cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a|$GF/notosans/OFL.txt
fonts/licenses/NotoSansJP.OFL.txt|1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9|$GF/notosansjp/OFL.txt
fonts/licenses/NotoSerifJP.OFL.txt|5e0da210fb04058a8c0087985d2d456b931c2579811a49655721d3cf0c36b6d6|$GF/notoserifjp/OFL.txt
fonts/licenses/JetBrainsMono.OFL.txt|b2fe5e8987594e9ffd1d2ca52a2f5d73eb8335243893c5d6254b5ad69269591d|$GF/jetbrainsmono/OFL.txt
fonts/licenses/STIXTwoMath.OFL.txt|0c8825913b60d858aacdb33c4ca6660a7d64b0d6464702efbb19313f5765861a|$GF/stixtwomath/OFL.txt
fonts/licenses/STIXTwoText.OFL.txt|62c0967a997b9691326d35b1f90baf085a557327b711618bc4161325ce1bb1b2|$GF/stixtwotext/OFL.txt
fonts/licenses/SourceHanCodeJP.LICENSE.txt|6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2|$SHCJ_RAW/LICENSE.txt
csl/ieee.csl|b4c7619fc16c45a31e4cc3271eab94ffe83192d3b4c7fc729470a3b459448de3|$STYLES/ieee.csl
csl/locales-ja-JP.xml|5167d0686946405a0531c484b61a6e0e43b652631443f06908b3396ce6ffb543|$LOCALES/locales-ja-JP.xml"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetched=0
skipped=0
while IFS='|' read -r rel expected url; do
  dest="$ROOT/vendor/$rel"
  if [ -f "$dest" ]; then
    actual="$(sha256_of "$dest")"
    if [ "$actual" = "$expected" ]; then
      skipped=$((skipped + 1))
      continue
    fi
    echo "error: vendor/$rel のハッシュがマニフェストと一致しません（手動変更または破損）。" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    echo "  再取得するにはファイルを削除して再実行してください。" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$dest")"
  tmp="$dest.download"
  echo "fetch vendor/$rel"
  curl -fsSL --retry 3 -o "$tmp" "$url"
  actual="$(sha256_of "$tmp")"
  if [ "$actual" != "$expected" ]; then
    rm -f "$tmp"
    echo "error: $url のダウンロード内容がピン留めハッシュと一致しません。" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
  mv "$tmp" "$dest"
  fetched=$((fetched + 1))
done <<<"$MANIFEST"

echo "done: fetched=$fetched skipped=$skipped -> $ROOT/vendor/"
