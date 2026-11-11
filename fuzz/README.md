# frontend fuzz

frontend（字句解析・構文解析・CST → HIR の評価）を [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html)
（libFuzzer）で探索する。フォント・画像・設定ファイルの読込や PDF 生成は対象外で、外部資源の準備は要らない。

proptest・fuzz・golden の使い分けは `docs/coding-conventions.md`「テスト」節が正典。この文書は実行手順だけを持つ。

## 何を失敗とみなすか

任意の UTF-8 入力について、成功と通常の診断エラー（構文エラー・評価エラー）はどちらも正常な結果。失敗は次だけ:

- panic・abort・stack overflow
- 不変条件の破れ（`seiran_compiler::fuzzing::check_source` が検査する。定義は
  `frontend::syntax::preservation`（トークンと CST の入力保存）と `frontend::hir_invariants::parse_checked`
  （HIR の決定性・`NodeId` の一意性と親子の順序・span の範囲と字の境界）、診断ラベルの範囲）
- 資源予算の超過（1 入力の時間・メモリ）

fuzz で壊れないことは、意味や組版結果の正しさの証明ではない。

## 準備

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
```

## 実行

```sh
tools/fuzz-frontend.sh [秒数] [libFuzzer の追加引数...]
```

既定は 60 秒。資源予算（いずれも探索の予算で、製品の入力上限ではない）:

| フラグ | 既定 | 意味 |
| --- | --- | --- |
| `-max_total_time` | 第 1 引数（60） | 探索全体の秒数 |
| `-timeout` | 10 | 1 入力の秒数。超えると `timeout-*` を保存 |
| `-rss_limit_mb` | 2048 | メモリ上限。超えると `oom-*` を保存 |
| `-max_len` | 8192 | 1 入力のバイト数（`tests/text/perf` の大きい fixture も切り詰めて読む） |

追加引数は後ろに付くので既定を上書きできる。並列は `-jobs=4 -workers=4`、クラッシュ後も探索を続けるなら
`-fork=1 -ignore_crashes=1`。

## 評価段への到達の確認

構文エラーだけを繰り返す corpus は評価段を探索しない。探索後に到達段を数える:

```sh
cargo +nightly run -p seiran-fuzz --example corpus_stats -- fuzz/corpus/frontend
```

`Evaluated` と `EvalError` の合計が corpus の 10% 未満なら、`fuzz/seeds/frontend` に評価まで届く seed（`ok-`）を
足すか辞書を見直す。

## 失敗入力

保存先は `fuzz/artifacts/frontend/{crash,timeout,oom}-<hash>`。単独再実行は探索と同じスクリプトを使う:

```sh
tools/fuzz-frontend.sh --replay fuzz/artifacts/frontend/crash-<hash>
```

再実行にはファイルを指定する。既定の資源上限は探索と共通で、後ろに libFuzzer の追加引数を付けて上書きできる。
単独再実行と最小化には**発見時と同じ上限**を渡す。
`cargo fuzz` を直接叩くときは、スクリプトと同じく release profile の LTO と strip を外す（root の LTO は Linux で
ASan の link を壊す。スクリプトと同じビルドを再利用できる）:

```sh
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_STRIP=false
cargo +nightly fuzz run frontend fuzz/artifacts/frontend/crash-<hash> -- -timeout=10 -rss_limit_mb=2048 -max_len=8192
cargo +nightly fuzz tmin frontend fuzz/artifacts/frontend/crash-<hash> -- -timeout=10 -rss_limit_mb=2048
```

libFuzzer の `-timeout` の既定は 1200 秒なので、省略すると 10 秒超過で見つけた入力が成功扱いになる。`tmin` にだけは
`-max_len` を渡さない（libFuzzer の `-minimize_crash` が内部で入力長を設定し直すため、渡すと
`Assertion failed: (this->MaxInputLen == 0)` で止まる。縮めるだけなので入力長の上限は要らない）。探索時に
上限を変えたなら、その値をログと issue に残す。最小化できなかったときも元の artifact と実行条件を保存する。

調査の結果が実装の不具合なら、最小化した入力と期待結果を添えて issue を立てる。修正時は期待結果を明記した通常の
回帰テストへ入力を残し、一般化できる性質が見つかったら proptest の追加・生成規則の改善も検討する。
`Span::DUMMY` の検査などの不変条件が破れたときは、不変条件を緩めず、実装の不具合か仕様かを issue で判断する。

## corpus

| 場所 | 中身 | commit |
| --- | --- | --- |
| `fuzz/seeds/frontend` | 手で用意した初期 corpus | する |
| `fuzz/corpus/frontend` | 探索で増えた入力（libFuzzer は先頭のディレクトリだけに書く） | しない（次回の実行が再利用する） |
| `tests/text` | 既存の fixture（正常例・診断用）。読むだけ | 既存 |

seed の名前の接頭辞は到達すべき段を表す: `ok-` は評価成功、`syntax-` は構文エラー、`eval-` は評価エラー、`any-` は
期待なし（panic しないことだけ）。通常テスト `seeds_reach_the_outcome_named_by_their_prefix`
（`crates/seiran-compiler/src/frontend/fuzzing.rs`）が分類を検査するので、seed を足したら `cargo test --all-features`
を通す。探索で見つかった入力のうち残す価値があるものは、接頭辞を付けて seed へ commit する。

corpus が膨らんだら縮める（「失敗入力」節と同じく LTO と strip を外して叩く）:

```sh
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_STRIP=false
cargo +nightly fuzz cmin frontend fuzz/corpus/frontend
```

## 辞書

`fuzz/frontend.dict` は構文の字面とレジストリのコマンド名・環境名・数式記号名の libFuzzer 辞書
（[形式](https://llvm.org/docs/LibFuzzer.html#dictionaries)。`\` は `\\`、制御文字は `\xHH`）。探索の手掛かりで、
新コマンドが漏れても誤検出にはならず効率が落ちるだけ。再生成:

```sh
E=crates/seiran-compiler/src/frontend/evaluator
{
  printf '%s\n' '"\\"' '"{"' '"}"' '"["' '"]"' '"$"' '"_"' '"^"' '"&"' '","' '"="' '"//"' '"\\\\"' \
    '"\\{"' '"\\}"' '"\\$"' '"\\,"' '"\\="' '"\x0A\x0A"' '"\x0D\x0A"'
  perl -ne 'print "\"\\\\$1\"\n" if /^\s*"([A-Za-z]+)"\s*=>/' $E/command.rs $E/command/symbol.rs $E/math.rs
  perl -ne 'print "\"\\\\begin{$1}\"\n\"\\\\end{$1}\"\n" if /^\s*"([A-Za-z]+)"\s*=>/' $E/environment.rs
} | LC_ALL=C sort -u > fuzz/frontend.dict
```

## CI

`.github/workflows/fuzz.yml` が週 2 回（水曜・日曜 03:00 JST、1800 秒）と手動起動（既定 900 秒）で回す。既知のクラッシュで探索が
止まらないよう `-fork=2 -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1` で最後まで回し、終わった後に
`fuzz/artifacts/frontend` に失敗入力が 1 つでもあれば job を失敗させる（未修正の既知の不具合があれば毎回失敗する）:

```sh
gh workflow run fuzz.yml -f seconds=600
```

Actions の "Fuzz" → Run workflow からも起動できる。step summary に到達段の集計が出る。失敗時は失敗入力が
artifact `fuzz-artifacts`、ログは毎回 `fuzz-log` に載る。fork モードは子プロセスの詳細ログを残さないので、探索後に
失敗入力を同じ資源上限で個別に再実行する。`fuzz-log` は探索ログ `fuzz-frontend.log` に加え、入力ごとの標準出力・
標準エラー・終了コードを `fuzz-replay-logs/<失敗入力名>.log` に含む。再実行が異常終了しても残りの入力を処理し、
再実行で再現しなくても失敗入力があれば job は失敗する。手元での再現:

```sh
gh run download <run-id> -n fuzz-artifacts -D fuzz/artifacts/frontend
```

の後、「失敗入力」節の手順で再実行・最小化する。corpus は Actions cache が run をまたいで引き継ぐ（失敗した run でも
保存する）。7 日アクセスが無いと消え、そのときは seed から再出発する（週 2 回にしているのはこのため）。
