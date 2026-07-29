//! カウンタの値（構造のみ）。表示文字列の生成は typeset 側の責務
//!
//! [`CounterValue`] は `resets` / `reset_by`（値に影響する style フィールド）だけから
//! 組み立てる。`number_format` 等の表示側フィールドはこのクレートが一切読まないことで、
//! G3（内容は見た目から独立）を型の設計として保証する。

use config::CounterName;
use model::TheoremClass;

/// カウンタの種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CounterKind {
  /// `config::Counters` が定義する固定 9 種のいずれか
  Counter(CounterName),
  /// 定理クラス（共有カウンタは複数クラスが 1 つを共有しうる）
  Theorem(TheoremClass),
}

/// カウンタの値（構造のみ）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterValue {
  /// このカウンタの種別
  pub kind: CounterKind,
  /// 祖先カウンタから自身までの値列（末尾が自身の値）
  pub parts: Vec<u32>,
}
