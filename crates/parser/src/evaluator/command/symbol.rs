//! 数式記号テーブル
//!
//! 引数なしで単一の Unicode 文字を出力する記号コマンド（ギリシャ文字・数学記号）を
//! [`SYMBOL_MAP`] に集約する。各記号は出力文字 [`MathSymbol::ch`] と
//! 数式クラス [`MathSymbol::class`] を持つ。
//!
//! ## 機能コマンドとの分離
//!
//! 制御・書体・色・見出し・参照・リンクといった**機能コマンド**は
//! [`crate::evaluator::command::COMMAND_MAP`] に置き、本モジュールには
//! **記号**だけを置く。コマンド解決は `COMMAND_MAP` を引いた後 miss なら
//! `SYMBOL_MAP` を引く（機能コマンド名が記号名に優先する）。両マップにキー重複が
//! ないことはテスト（`no_key_collision_between_command_and_symbol_maps`）で保証する。
//!
//! ## 数式クラスの位置づけ
//!
//! 各記号の [`MathClass`] は上流 `unicode-math-table.tex` の `\mathord` / `\mathbin` 等から
//! 取得して記録するのみで、**本モジュールではスペーシングを実装しない**
//! （`MathNode::Symbol` は `char` のまま）。クラスを今データに持たせておくことで、
//! 将来の数式スペーシング実装が記号表を作り直さずに消費できる。
//!
//! ## 命名規則（構文設計原則 B・分かりやすさ第一）
//!
//! コマンド名は unicode-math の csname に一致させず、慣用の短縮形を優先して
//! Seiran 独自に決める（`\leq` `\subseteq` `\rightarrow`）。冗長で長すぎる名前は避ける。

use phf::phf_map;
use types::MathClass;

/// 記号コマンドが出力する単一文字とその数式クラス
///
/// [`SYMBOL_MAP`] の値として使い、記号の Unicode 文字（[`ch`](Self::ch)）と
/// 数式クラス（[`class`](Self::class)）を対にして保持する。
#[derive(Clone, Copy, Debug)]
pub(crate) struct MathSymbol {
  /// 出力する Unicode 文字
  pub(crate) ch: char,
  /// 数式クラス（スペーシング用に記録、現状は未消費）
  ///
  /// 本 issue（#26）ではスペーシングを実装しないため、クラスはテーブルに記録するだけで
  /// 本体コードからは読まれない（テストでは検証する）。将来の数式スペーシング実装
  /// （#83 配下）がこのフィールドを消費する想定のため、`dead_code` を意図的に許容する。
  #[allow(dead_code)]
  pub(crate) class: MathClass,
}

impl MathSymbol {
  /// 記号エントリを生成する（[`SYMBOL_MAP`] 初期化用の const コンストラクタ）
  const fn new(ch: char, class: MathClass) -> Self { return Self { ch, class }; }
}

/// 記号コマンド名 → [`MathSymbol`] のパーフェクトハッシュマップ
///
/// ギリシャ文字と数学記号を数式クラス付きで集約する。記号の追加は本マップだけを
/// 編集すれば、本文・数式の両文脈（`Evaluator::evaluate_command` /
/// `extract_inline_nodes` / `resolve_symbol_command`）に反映される。
pub(crate) static SYMBOL_MAP: phf::Map<&'static str, MathSymbol> = phf_map! {
  // ───────────────────────────── ギリシャ文字（大文字, Ord）─────────────────────────────
  "Alpha" => MathSymbol::new('\u{0391}', MathClass::Ord),
  "Beta" => MathSymbol::new('\u{0392}', MathClass::Ord),
  "Gamma" => MathSymbol::new('\u{0393}', MathClass::Ord),
  "Delta" => MathSymbol::new('\u{0394}', MathClass::Ord),
  "Epsilon" => MathSymbol::new('\u{0395}', MathClass::Ord),
  "Zeta" => MathSymbol::new('\u{0396}', MathClass::Ord),
  "Eta" => MathSymbol::new('\u{0397}', MathClass::Ord),
  "Theta" => MathSymbol::new('\u{0398}', MathClass::Ord),
  "Iota" => MathSymbol::new('\u{0399}', MathClass::Ord),
  "Kappa" => MathSymbol::new('\u{039A}', MathClass::Ord),
  "Lambda" => MathSymbol::new('\u{039B}', MathClass::Ord),
  "Mu" => MathSymbol::new('\u{039C}', MathClass::Ord),
  "Nu" => MathSymbol::new('\u{039D}', MathClass::Ord),
  "Xi" => MathSymbol::new('\u{039E}', MathClass::Ord),
  "Omicron" => MathSymbol::new('\u{039F}', MathClass::Ord),
  "Pi" => MathSymbol::new('\u{03A0}', MathClass::Ord),
  "Rho" => MathSymbol::new('\u{03A1}', MathClass::Ord),
  "Sigma" => MathSymbol::new('\u{03A3}', MathClass::Ord),
  "Tau" => MathSymbol::new('\u{03A4}', MathClass::Ord),
  "Upsilon" => MathSymbol::new('\u{03A5}', MathClass::Ord),
  "Phi" => MathSymbol::new('\u{03A6}', MathClass::Ord),
  "Chi" => MathSymbol::new('\u{03A7}', MathClass::Ord),
  "Psi" => MathSymbol::new('\u{03A8}', MathClass::Ord),
  "Omega" => MathSymbol::new('\u{03A9}', MathClass::Ord),

  // ───────────────────────────── ギリシャ文字（小文字, Ord）─────────────────────────────
  "alpha" => MathSymbol::new('\u{03B1}', MathClass::Ord),
  "beta" => MathSymbol::new('\u{03B2}', MathClass::Ord),
  "gamma" => MathSymbol::new('\u{03B3}', MathClass::Ord),
  "delta" => MathSymbol::new('\u{03B4}', MathClass::Ord),
  "epsilon" => MathSymbol::new('\u{03B5}', MathClass::Ord),
  "varepsilon" => MathSymbol::new('\u{03F5}', MathClass::Ord),
  "zeta" => MathSymbol::new('\u{03B6}', MathClass::Ord),
  "eta" => MathSymbol::new('\u{03B7}', MathClass::Ord),
  "theta" => MathSymbol::new('\u{03B8}', MathClass::Ord),
  "vartheta" => MathSymbol::new('\u{03D1}', MathClass::Ord),
  "iota" => MathSymbol::new('\u{03B9}', MathClass::Ord),
  "kappa" => MathSymbol::new('\u{03BA}', MathClass::Ord),
  "varkappa" => MathSymbol::new('\u{03F0}', MathClass::Ord),
  "lambda" => MathSymbol::new('\u{03BB}', MathClass::Ord),
  "mu" => MathSymbol::new('\u{03BC}', MathClass::Ord),
  "nu" => MathSymbol::new('\u{03BD}', MathClass::Ord),
  "xi" => MathSymbol::new('\u{03BE}', MathClass::Ord),
  "omicron" => MathSymbol::new('\u{03BF}', MathClass::Ord),
  "pi" => MathSymbol::new('\u{03C0}', MathClass::Ord),
  "varpi" => MathSymbol::new('\u{03D6}', MathClass::Ord),
  "rho" => MathSymbol::new('\u{03C1}', MathClass::Ord),
  "varrho" => MathSymbol::new('\u{03F1}', MathClass::Ord),
  "sigma" => MathSymbol::new('\u{03C3}', MathClass::Ord),
  "varsigma" => MathSymbol::new('\u{03C2}', MathClass::Ord),
  "tau" => MathSymbol::new('\u{03C4}', MathClass::Ord),
  "upsilon" => MathSymbol::new('\u{03C5}', MathClass::Ord),
  "phi" => MathSymbol::new('\u{03C6}', MathClass::Ord),
  "varphi" => MathSymbol::new('\u{03D5}', MathClass::Ord),
  "chi" => MathSymbol::new('\u{03C7}', MathClass::Ord),
  "psi" => MathSymbol::new('\u{03C8}', MathClass::Ord),
  "omega" => MathSymbol::new('\u{03C9}', MathClass::Ord),

  // ───────────────────────────── 名前付き記号・定数（Ord）─────────────────────────────
  "forall" => MathSymbol::new('\u{2200}', MathClass::Ord),
  "complement" => MathSymbol::new('\u{2201}', MathClass::Ord),
  "partial" => MathSymbol::new('\u{2202}', MathClass::Ord),
  "exists" => MathSymbol::new('\u{2203}', MathClass::Ord),
  "notexists" => MathSymbol::new('\u{2204}', MathClass::Ord),
  "emptyset" => MathSymbol::new('\u{2205}', MathClass::Ord),
  "increment" => MathSymbol::new('\u{2206}', MathClass::Ord),
  "nabla" => MathSymbol::new('\u{2207}', MathClass::Ord),
  "qed" => MathSymbol::new('\u{220E}', MathClass::Ord),
  "surd" => MathSymbol::new('\u{221A}', MathClass::Ord),
  "infty" => MathSymbol::new('\u{221E}', MathClass::Ord),
  "rightangle" => MathSymbol::new('\u{221F}', MathClass::Ord),
  "angle" => MathSymbol::new('\u{2220}', MathClass::Ord),
  "measuredangle" => MathSymbol::new('\u{2221}', MathClass::Ord),
  "sphericalangle" => MathSymbol::new('\u{2222}', MathClass::Ord),
  "therefore" => MathSymbol::new('\u{2234}', MathClass::Ord),
  "because" => MathSymbol::new('\u{2235}', MathClass::Ord),
  "neg" => MathSymbol::new('\u{00AC}', MathClass::Ord),
  "hbar" => MathSymbol::new('\u{210F}', MathClass::Ord),
  "ell" => MathSymbol::new('\u{2113}', MathClass::Ord),
  "wp" => MathSymbol::new('\u{2118}', MathClass::Ord),
  "Re" => MathSymbol::new('\u{211C}', MathClass::Ord),
  "Im" => MathSymbol::new('\u{2111}', MathClass::Ord),
  "aleph" => MathSymbol::new('\u{2135}', MathClass::Ord),
  "beth" => MathSymbol::new('\u{2136}', MathClass::Ord),
  "gimel" => MathSymbol::new('\u{2137}', MathClass::Ord),
  "daleth" => MathSymbol::new('\u{2138}', MathClass::Ord),
  "mho" => MathSymbol::new('\u{2127}', MathClass::Ord),
  "Finv" => MathSymbol::new('\u{2132}', MathClass::Ord),
  "Game" => MathSymbol::new('\u{2141}', MathClass::Ord),
  "eth" => MathSymbol::new('\u{00F0}', MathClass::Ord),
  "prime" => MathSymbol::new('\u{2032}', MathClass::Ord),
  "backprime" => MathSymbol::new('\u{2035}', MathClass::Ord),
  "top" => MathSymbol::new('\u{22A4}', MathClass::Ord),
  "bot" => MathSymbol::new('\u{22A5}', MathClass::Ord),
  "triangle" => MathSymbol::new('\u{25B3}', MathClass::Ord),
  "blacktriangle" => MathSymbol::new('\u{25B2}', MathClass::Ord),
  "blacktriangledown" => MathSymbol::new('\u{25BC}', MathClass::Ord),
  "square" => MathSymbol::new('\u{25A1}', MathClass::Ord),
  "blacksquare" => MathSymbol::new('\u{25A0}', MathClass::Ord),
  "lozenge" => MathSymbol::new('\u{25CA}', MathClass::Ord),
  "blacklozenge" => MathSymbol::new('\u{29EB}', MathClass::Ord),
  "bigstar" => MathSymbol::new('\u{2605}', MathClass::Ord),
  "diagup" => MathSymbol::new('\u{2571}', MathClass::Ord),
  "diagdown" => MathSymbol::new('\u{2572}', MathClass::Ord),
  "sharp" => MathSymbol::new('\u{266F}', MathClass::Ord),
  "flat" => MathSymbol::new('\u{266D}', MathClass::Ord),
  "natural" => MathSymbol::new('\u{266E}', MathClass::Ord),
  "clubsuit" => MathSymbol::new('\u{2663}', MathClass::Ord),
  "diamondsuit" => MathSymbol::new('\u{2662}', MathClass::Ord),
  "heartsuit" => MathSymbol::new('\u{2661}', MathClass::Ord),
  "spadesuit" => MathSymbol::new('\u{2660}', MathClass::Ord),
  "checkmark" => MathSymbol::new('\u{2713}', MathClass::Ord),
  "maltese" => MathSymbol::new('\u{2720}', MathClass::Ord),
  "circledR" => MathSymbol::new('\u{00AE}', MathClass::Ord),
  "circledS" => MathSymbol::new('\u{24C8}', MathClass::Ord),
  "ldots" => MathSymbol::new('\u{2026}', MathClass::Ord),
  "cdots" => MathSymbol::new('\u{22EF}', MathClass::Ord),
  "vdots" => MathSymbol::new('\u{22EE}', MathClass::Ord),
  "ddots" => MathSymbol::new('\u{22F1}', MathClass::Ord),

  // ───────────────────────────── 大型演算子（Op）─────────────────────────────
  "prod" => MathSymbol::new('\u{220F}', MathClass::Op),
  "coprod" => MathSymbol::new('\u{2210}', MathClass::Op),
  "sum" => MathSymbol::new('\u{2211}', MathClass::Op),
  "int" => MathSymbol::new('\u{222B}', MathClass::Op),
  "iint" => MathSymbol::new('\u{222C}', MathClass::Op),
  "iiint" => MathSymbol::new('\u{222D}', MathClass::Op),
  "oint" => MathSymbol::new('\u{222E}', MathClass::Op),
  "oiint" => MathSymbol::new('\u{222F}', MathClass::Op),
  "oiiint" => MathSymbol::new('\u{2230}', MathClass::Op),
  "bigcap" => MathSymbol::new('\u{22C2}', MathClass::Op),
  "bigcup" => MathSymbol::new('\u{22C3}', MathClass::Op),
  "bigsqcup" => MathSymbol::new('\u{2A06}', MathClass::Op),
  "bigvee" => MathSymbol::new('\u{22C1}', MathClass::Op),
  "bigwedge" => MathSymbol::new('\u{22C0}', MathClass::Op),
  "bigodot" => MathSymbol::new('\u{2A00}', MathClass::Op),
  "bigoplus" => MathSymbol::new('\u{2A01}', MathClass::Op),
  "bigotimes" => MathSymbol::new('\u{2A02}', MathClass::Op),
  "biguplus" => MathSymbol::new('\u{2A04}', MathClass::Op),

  // ───────────────────────────── 二項演算子（Bin）─────────────────────────────
  "minus" => MathSymbol::new('\u{2212}', MathClass::Bin),
  "mp" => MathSymbol::new('\u{2213}', MathClass::Bin),
  "pm" => MathSymbol::new('\u{00B1}', MathClass::Bin),
  "dotplus" => MathSymbol::new('\u{2214}', MathClass::Bin),
  "slash" => MathSymbol::new('\u{2215}', MathClass::Bin),
  "times" => MathSymbol::new('\u{00D7}', MathClass::Bin),
  "div" => MathSymbol::new('\u{00F7}', MathClass::Bin),
  "cdot" => MathSymbol::new('\u{22C5}', MathClass::Bin),
  "ast" => MathSymbol::new('\u{2217}', MathClass::Bin),
  "star" => MathSymbol::new('\u{22C6}', MathClass::Bin),
  "circ" => MathSymbol::new('\u{2218}', MathClass::Bin),
  "bullet" => MathSymbol::new('\u{2219}', MathClass::Bin),
  "diamond" => MathSymbol::new('\u{22C4}', MathClass::Bin),
  "setminus" => MathSymbol::new('\u{2216}', MathClass::Bin),
  "land" => MathSymbol::new('\u{2227}', MathClass::Bin),
  "lor" => MathSymbol::new('\u{2228}', MathClass::Bin),
  "wedge" => MathSymbol::new('\u{2227}', MathClass::Bin),
  "vee" => MathSymbol::new('\u{2228}', MathClass::Bin),
  "cap" => MathSymbol::new('\u{2229}', MathClass::Bin),
  "cup" => MathSymbol::new('\u{222A}', MathClass::Bin),
  "sqcap" => MathSymbol::new('\u{2293}', MathClass::Bin),
  "sqcup" => MathSymbol::new('\u{2294}', MathClass::Bin),
  "uplus" => MathSymbol::new('\u{228E}', MathClass::Bin),
  "amalg" => MathSymbol::new('\u{2A3F}', MathClass::Bin),
  "wr" => MathSymbol::new('\u{2240}', MathClass::Bin),
  "oplus" => MathSymbol::new('\u{2295}', MathClass::Bin),
  "ominus" => MathSymbol::new('\u{2296}', MathClass::Bin),
  "otimes" => MathSymbol::new('\u{2297}', MathClass::Bin),
  "oslash" => MathSymbol::new('\u{2298}', MathClass::Bin),
  "odot" => MathSymbol::new('\u{2299}', MathClass::Bin),
  "boxplus" => MathSymbol::new('\u{229E}', MathClass::Bin),
  "boxminus" => MathSymbol::new('\u{229F}', MathClass::Bin),
  "boxtimes" => MathSymbol::new('\u{22A0}', MathClass::Bin),
  "boxdot" => MathSymbol::new('\u{22A1}', MathClass::Bin),
  "triangleleft" => MathSymbol::new('\u{25C1}', MathClass::Bin),
  "triangleright" => MathSymbol::new('\u{25B7}', MathClass::Bin),
  "bigtriangleup" => MathSymbol::new('\u{25B3}', MathClass::Bin),
  "bigtriangledown" => MathSymbol::new('\u{25BD}', MathClass::Bin),
  "dagger" => MathSymbol::new('\u{2020}', MathClass::Bin),
  "ddagger" => MathSymbol::new('\u{2021}', MathClass::Bin),
  "ltimes" => MathSymbol::new('\u{22C9}', MathClass::Bin),
  "rtimes" => MathSymbol::new('\u{22CA}', MathClass::Bin),
  "barwedge" => MathSymbol::new('\u{22BC}', MathClass::Bin),
  "veebar" => MathSymbol::new('\u{22BB}', MathClass::Bin),
  "intercal" => MathSymbol::new('\u{22BA}', MathClass::Bin),
  "Cap" => MathSymbol::new('\u{22D2}', MathClass::Bin),
  "Cup" => MathSymbol::new('\u{22D3}', MathClass::Bin),

  // ───────────────────────────── 関係子（Rel）─────────────────────────────
  "in" => MathSymbol::new('\u{2208}', MathClass::Rel),
  "notin" => MathSymbol::new('\u{2209}', MathClass::Rel),
  "ni" => MathSymbol::new('\u{220B}', MathClass::Rel),
  "notni" => MathSymbol::new('\u{220C}', MathClass::Rel),
  "propto" => MathSymbol::new('\u{221D}', MathClass::Rel),
  "mid" => MathSymbol::new('\u{2223}', MathClass::Rel),
  "nmid" => MathSymbol::new('\u{2224}', MathClass::Rel),
  "parallel" => MathSymbol::new('\u{2225}', MathClass::Rel),
  "notparallel" => MathSymbol::new('\u{2226}', MathClass::Rel),
  "backsim" => MathSymbol::new('\u{223D}', MathClass::Rel),
  "sim" => MathSymbol::new('\u{223C}', MathClass::Rel),
  "nsim" => MathSymbol::new('\u{2241}', MathClass::Rel),
  "simeq" => MathSymbol::new('\u{2243}', MathClass::Rel),
  "approx" => MathSymbol::new('\u{2248}', MathClass::Rel),
  "approxeq" => MathSymbol::new('\u{224A}', MathClass::Rel),
  "asymp" => MathSymbol::new('\u{224D}', MathClass::Rel),
  "cong" => MathSymbol::new('\u{2245}', MathClass::Rel),
  "ncong" => MathSymbol::new('\u{2247}', MathClass::Rel),
  "doteq" => MathSymbol::new('\u{2250}', MathClass::Rel),
  "triangleq" => MathSymbol::new('\u{225C}', MathClass::Rel),
  "neq" => MathSymbol::new('\u{2260}', MathClass::Rel),
  "equiv" => MathSymbol::new('\u{2261}', MathClass::Rel),
  "leq" => MathSymbol::new('\u{2264}', MathClass::Rel),
  "geq" => MathSymbol::new('\u{2265}', MathClass::Rel),
  "ll" => MathSymbol::new('\u{226A}', MathClass::Rel),
  "gg" => MathSymbol::new('\u{226B}', MathClass::Rel),
  "nless" => MathSymbol::new('\u{226E}', MathClass::Rel),
  "ngtr" => MathSymbol::new('\u{226F}', MathClass::Rel),
  "nleq" => MathSymbol::new('\u{2270}', MathClass::Rel),
  "ngeq" => MathSymbol::new('\u{2271}', MathClass::Rel),
  "lesssim" => MathSymbol::new('\u{2272}', MathClass::Rel),
  "gtrsim" => MathSymbol::new('\u{2273}', MathClass::Rel),
  "lessgtr" => MathSymbol::new('\u{2276}', MathClass::Rel),
  "gtrless" => MathSymbol::new('\u{2277}', MathClass::Rel),
  "prec" => MathSymbol::new('\u{227A}', MathClass::Rel),
  "succ" => MathSymbol::new('\u{227B}', MathClass::Rel),
  "preceq" => MathSymbol::new('\u{227C}', MathClass::Rel),
  "succeq" => MathSymbol::new('\u{227D}', MathClass::Rel),
  "nprec" => MathSymbol::new('\u{2280}', MathClass::Rel),
  "nsucc" => MathSymbol::new('\u{2281}', MathClass::Rel),
  "subset" => MathSymbol::new('\u{2282}', MathClass::Rel),
  "supset" => MathSymbol::new('\u{2283}', MathClass::Rel),
  "subseteq" => MathSymbol::new('\u{2286}', MathClass::Rel),
  "supseteq" => MathSymbol::new('\u{2287}', MathClass::Rel),
  "nsubseteq" => MathSymbol::new('\u{2288}', MathClass::Rel),
  "nsupseteq" => MathSymbol::new('\u{2289}', MathClass::Rel),
  "subsetneq" => MathSymbol::new('\u{228A}', MathClass::Rel),
  "supsetneq" => MathSymbol::new('\u{228B}', MathClass::Rel),
  "sqsubset" => MathSymbol::new('\u{228F}', MathClass::Rel),
  "sqsupset" => MathSymbol::new('\u{2290}', MathClass::Rel),
  "sqsubseteq" => MathSymbol::new('\u{2291}', MathClass::Rel),
  "sqsupseteq" => MathSymbol::new('\u{2292}', MathClass::Rel),
  "vdash" => MathSymbol::new('\u{22A2}', MathClass::Rel),
  "dashv" => MathSymbol::new('\u{22A3}', MathClass::Rel),
  "perp" => MathSymbol::new('\u{22A5}', MathClass::Rel),
  "models" => MathSymbol::new('\u{22A8}', MathClass::Rel),
  "nvdash" => MathSymbol::new('\u{22AC}', MathClass::Rel),

  // ───────────────────────────── 矢印（Rel）─────────────────────────────
  "leftarrow" => MathSymbol::new('\u{2190}', MathClass::Rel),
  "uparrow" => MathSymbol::new('\u{2191}', MathClass::Rel),
  "rightarrow" => MathSymbol::new('\u{2192}', MathClass::Rel),
  "downarrow" => MathSymbol::new('\u{2193}', MathClass::Rel),
  "leftrightarrow" => MathSymbol::new('\u{2194}', MathClass::Rel),
  "updownarrow" => MathSymbol::new('\u{2195}', MathClass::Rel),
  "nwarrow" => MathSymbol::new('\u{2196}', MathClass::Rel),
  "nearrow" => MathSymbol::new('\u{2197}', MathClass::Rel),
  "searrow" => MathSymbol::new('\u{2198}', MathClass::Rel),
  "swarrow" => MathSymbol::new('\u{2199}', MathClass::Rel),
  "nleftarrow" => MathSymbol::new('\u{219A}', MathClass::Rel),
  "nrightarrow" => MathSymbol::new('\u{219B}', MathClass::Rel),
  "twoheadleftarrow" => MathSymbol::new('\u{219E}', MathClass::Rel),
  "twoheadrightarrow" => MathSymbol::new('\u{21A0}', MathClass::Rel),
  "hookleftarrow" => MathSymbol::new('\u{21A9}', MathClass::Rel),
  "hookrightarrow" => MathSymbol::new('\u{21AA}', MathClass::Rel),
  "mapsto" => MathSymbol::new('\u{21A6}', MathClass::Rel),
  "nleftrightarrow" => MathSymbol::new('\u{21AE}', MathClass::Rel),
  "leftharpoonup" => MathSymbol::new('\u{21BC}', MathClass::Rel),
  "leftharpoondown" => MathSymbol::new('\u{21BD}', MathClass::Rel),
  "rightharpoonup" => MathSymbol::new('\u{21C0}', MathClass::Rel),
  "rightharpoondown" => MathSymbol::new('\u{21C1}', MathClass::Rel),
  "rightleftarrows" => MathSymbol::new('\u{21C4}', MathClass::Rel),
  "leftrightarrows" => MathSymbol::new('\u{21C6}', MathClass::Rel),
  "leftleftarrows" => MathSymbol::new('\u{21C7}', MathClass::Rel),
  "upuparrows" => MathSymbol::new('\u{21C8}', MathClass::Rel),
  "rightrightarrows" => MathSymbol::new('\u{21C9}', MathClass::Rel),
  "downdownarrows" => MathSymbol::new('\u{21CA}', MathClass::Rel),
  "rightleftharpoons" => MathSymbol::new('\u{21CC}', MathClass::Rel),
  "curvearrowleft" => MathSymbol::new('\u{21B6}', MathClass::Rel),
  "curvearrowright" => MathSymbol::new('\u{21B7}', MathClass::Rel),
  "circlearrowleft" => MathSymbol::new('\u{21BA}', MathClass::Rel),
  "circlearrowright" => MathSymbol::new('\u{21BB}', MathClass::Rel),
  "Leftarrow" => MathSymbol::new('\u{21D0}', MathClass::Rel),
  "Uparrow" => MathSymbol::new('\u{21D1}', MathClass::Rel),
  "Rightarrow" => MathSymbol::new('\u{21D2}', MathClass::Rel),
  "Downarrow" => MathSymbol::new('\u{21D3}', MathClass::Rel),
  "Leftrightarrow" => MathSymbol::new('\u{21D4}', MathClass::Rel),
  "Updownarrow" => MathSymbol::new('\u{21D5}', MathClass::Rel),
  "nLeftarrow" => MathSymbol::new('\u{21CD}', MathClass::Rel),
  "nLeftrightarrow" => MathSymbol::new('\u{21CE}', MathClass::Rel),
  "nRightarrow" => MathSymbol::new('\u{21CF}', MathClass::Rel),
  "longleftarrow" => MathSymbol::new('\u{27F5}', MathClass::Rel),
  "longrightarrow" => MathSymbol::new('\u{27F6}', MathClass::Rel),
  "longleftrightarrow" => MathSymbol::new('\u{27F7}', MathClass::Rel),
  "Longleftarrow" => MathSymbol::new('\u{27F8}', MathClass::Rel),
  "Longrightarrow" => MathSymbol::new('\u{27F9}', MathClass::Rel),
  "Longleftrightarrow" => MathSymbol::new('\u{27FA}', MathClass::Rel),
  "longmapsto" => MathSymbol::new('\u{27FC}', MathClass::Rel),

  // ───────────────────────────── 開き / 閉じ括弧（Open / Close）─────────────────────────────
  "langle" => MathSymbol::new('\u{27E8}', MathClass::Open),
  "rangle" => MathSymbol::new('\u{27E9}', MathClass::Close),
  "lceil" => MathSymbol::new('\u{2308}', MathClass::Open),
  "rceil" => MathSymbol::new('\u{2309}', MathClass::Close),
  "lfloor" => MathSymbol::new('\u{230A}', MathClass::Open),
  "rfloor" => MathSymbol::new('\u{230B}', MathClass::Close),
  "vert" => MathSymbol::new('\u{007C}', MathClass::Ord),
  "Vert" => MathSymbol::new('\u{2016}', MathClass::Ord),
};

#[cfg(test)]
mod tests {
  use super::SYMBOL_MAP;
  use crate::evaluator::command::COMMAND_MAP;

  #[test]
  fn representative_symbols_have_expected_class() {
    // Arrange & Act & Assert — 代表記号のクラスが各カテゴリで正しいこと
    use types::MathClass;
    assert_eq!(SYMBOL_MAP.get("alpha").map(|s| s.class), Some(MathClass::Ord));
    assert_eq!(SYMBOL_MAP.get("leq").map(|s| s.class), Some(MathClass::Rel));
    assert_eq!(SYMBOL_MAP.get("times").map(|s| s.class), Some(MathClass::Bin));
    assert_eq!(SYMBOL_MAP.get("sum").map(|s| s.class), Some(MathClass::Op));
    assert_eq!(SYMBOL_MAP.get("rightarrow").map(|s| s.class), Some(MathClass::Rel));
    assert_eq!(SYMBOL_MAP.get("langle").map(|s| s.class), Some(MathClass::Open));
    assert_eq!(SYMBOL_MAP.get("rangle").map(|s| s.class), Some(MathClass::Close));
  }

  #[test]
  fn representative_symbols_map_to_expected_char() {
    // Arrange & Act & Assert — 移設した既存記号・新規 amssymb 記号が正しい文字に解決される
    assert_eq!(SYMBOL_MAP.get("alpha").map(|s| s.ch), Some('\u{03B1}'));
    assert_eq!(SYMBOL_MAP.get("leq").map(|s| s.ch), Some('\u{2264}'));
    assert_eq!(SYMBOL_MAP.get("geq").map(|s| s.ch), Some('\u{2265}'));
    assert_eq!(SYMBOL_MAP.get("subseteq").map(|s| s.ch), Some('\u{2286}'));
    assert_eq!(SYMBOL_MAP.get("Rightarrow").map(|s| s.ch), Some('\u{21D2}'));
  }

  #[test]
  fn no_key_collision_between_command_and_symbol_maps() {
    // Arrange & Act & Assert — 機能コマンドと記号のキーは重複してはならない（解決順序の衝突ガード）
    for key in SYMBOL_MAP.keys() {
      assert!(!COMMAND_MAP.contains_key(key), "COMMAND_MAP と SYMBOL_MAP にキーが重複しています: {key}");
    }
  }
}
