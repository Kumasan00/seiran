//! ギリシャ文字・数学記号出力コマンド群
//!
//! パーサーから呼ばれるギリシャ文字・数学記号コマンドを提供します。すべての関数は
//! 単一文字の `InlineNode::Symbol` を生成します。

use crate::{ast::CommandView, document::InlineNode, evaluator::EvalError};

/// 単一の文字関数を生成するヘルパーマクロ
///
/// このマクロは指定されたUnicode文字を返す関数を生成します。
/// 生成される関数は引数なしでコマンドビューを受け取り、
/// 単一文字の `InlineNode::Symbol` を返します。
///
/// # エラーハンドリング
///
/// - 必須引数（`{}`）が指定された場合は [`EvalError::ExtraCommandArgument`] を返します
/// - 任意引数（`[]`）が指定された場合は [`EvalError::ExtraCommandArgument`] を返します
macro_rules! single_char {
  ($fn_name:ident, $doc:expr, $ch:expr) => {
    #[doc = concat!($doc, "\n\n# エラー\n\n引数が指定されている場合は [`EvalError::ExtraCommandArgument`] を返します。")]
    pub(super) fn $fn_name(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
      if !view.args_is_empty() || !view.opt_args_is_empty() {
        return Err(EvalError::ExtraCommandArgument {
          name: view.name().to_string(),
          span: view.span().into(),
        });
      }
      return Ok(vec![InlineNode::Symbol($ch)]);
    }
  };
}

single_char!(upper_alpha, "ギリシャ文字の大文字アルファを出力します。", '\u{0391}');
single_char!(upper_beta, "ギリシャ文字の大文字ベータを出力します。", '\u{0392}');
single_char!(upper_gamma, "ギリシャ文字の大文字ガンマを出力します。", '\u{0393}');
single_char!(upper_delta, "ギリシャ文字の大文字デルタを出力します。", '\u{0394}');
single_char!(upper_epsilon, "ギリシャ文字の大文字イプシロンを出力します。", '\u{0395}');
single_char!(upper_zeta, "ギリシャ文字の大文字ゼータを出力します。", '\u{0396}');
single_char!(upper_eta, "ギリシャ文字の大文字エータを出力します。", '\u{0397}');
single_char!(upper_theta, "ギリシャ文字の大文字シータを出力します。", '\u{0398}');
single_char!(upper_iota, "ギリシャ文字の大文字イオタを出力します。", '\u{0399}');
single_char!(upper_kappa, "ギリシャ文字の大文字カッパを出力します。", '\u{039A}');
single_char!(upper_lambda, "ギリシャ文字の大文字ラムダを出力します。", '\u{039B}');
single_char!(upper_mu, "ギリシャ文字の大文字ミューを出力します。", '\u{039C}');
single_char!(upper_nu, "ギリシャ文字の大文字ニューを出力します。", '\u{039D}');
single_char!(upper_xi, "ギリシャ文字の大文字クシーを出力します。", '\u{039E}');
single_char!(upper_omicron, "ギリシャ文字の大文字オミクロンを出力します。", '\u{039F}');
single_char!(upper_pi, "ギリシャ文字の大文字パイを出力します。", '\u{03A0}');
single_char!(upper_rho, "ギリシャ文字の大文字ローを出力します。", '\u{03A1}');
single_char!(upper_sigma, "ギリシャ文字の大文字シグマを出力します。", '\u{03A3}');
single_char!(upper_tau, "ギリシャ文字の大文字タウを出力します。", '\u{03A4}');
single_char!(upper_upsilon, "ギリシャ文字の大文字ユプシロンを出力します。", '\u{03A5}');
single_char!(upper_phi, "ギリシャ文字の大文字ファイを出力します。", '\u{03A6}');
single_char!(upper_chi, "ギリシャ文字の大文字カイを出力します。", '\u{03A7}');
single_char!(upper_psi, "ギリシャ文字の大文字プサイを出力します。", '\u{03A8}');
single_char!(upper_omega, "ギリシャ文字の大文字オメガを出力します。", '\u{03A9}');
single_char!(lower_alpha, "ギリシャ文字の小文字アルファを出力します。", '\u{03B1}');
single_char!(lower_beta, "ギリシャ文字の小文字ベータを出力します。", '\u{03B2}');
single_char!(lower_gamma, "ギリシャ文字の小文字ガンマを出力します。", '\u{03B3}');
single_char!(lower_delta, "ギリシャ文字の小文字デルタを出力します。", '\u{03B4}');
single_char!(lower_epsilon, "ギリシャ文字の小文字イプシロンを出力します。", '\u{03B5}');
single_char!(var_epsilon, "ギリシャ文字の小文字イプシロン（バリアント）を出力します。", '\u{03F5}');
single_char!(lower_zeta, "ギリシャ文字の小文字ゼータを出力します。", '\u{03B6}');
single_char!(lower_eta, "ギリシャ文字の小文字エータを出力します。", '\u{03B7}');
single_char!(lower_theta, "ギリシャ文字の小文字シータを出力します。", '\u{03B8}');
single_char!(var_theta, "ギリシャ文字の小文字シータ（バリアント）を出力します。", '\u{03D1}');
single_char!(lower_iota, "ギリシャ文字の小文字イオタを出力します。", '\u{03B9}');
single_char!(lower_kappa, "ギリシャ文字の小文字カッパを出力します。", '\u{03BA}');
single_char!(var_kappa, "ギリシャ文字の小文字カッパ（バリアント）を出力します。", '\u{03F0}');
single_char!(lower_lambda, "ギリシャ文字の小文字ラムダを出力します。", '\u{03BB}');
single_char!(lower_mu, "ギリシャ文字の小文字ミューを出力します。", '\u{03BC}');
single_char!(lower_nu, "ギリシャ文字の小文字ニューを出力します。", '\u{03BD}');
single_char!(lower_xi, "ギリシャ文字の小文字クシーを出力します。", '\u{03BE}');
single_char!(lower_omicron, "ギリシャ文字の小文字オミクロンを出力します。", '\u{03BF}');
single_char!(lower_pi, "ギリシャ文字の小文字パイを出力します。", '\u{03C0}');
single_char!(var_pi, "ギリシャ文字の小文字パイ（バリアント）を出力します。", '\u{03D6}');
single_char!(lower_rho, "ギリシャ文字の小文字ローを出力します。", '\u{03C1}');
single_char!(var_rho, "ギリシャ文字の小文字ロー（バリアント）を出力します。", '\u{03F1}');
single_char!(lower_sigma, "ギリシャ文字の小文字シグマを出力します。", '\u{03C3}');
single_char!(final_sigma, "ギリシャ文字の小文字シグマ（語末形）を出力します。", '\u{03C2}');
single_char!(lower_tau, "ギリシャ文字の小文字タウを出力します。", '\u{03C4}');
single_char!(lower_upsilon, "ギリシャ文字の小文字ユプシロンを出力します。", '\u{03C5}');
single_char!(lower_phi, "ギリシャ文字の小文字ファイを出力します。", '\u{03C6}');
single_char!(lower_chi, "ギリシャ文字の小文字カイを出力します。", '\u{03C7}');
single_char!(lower_psi, "ギリシャ文字の小文字プサイを出力します。", '\u{03C8}');
single_char!(lower_omega, "ギリシャ文字の小文字オメガを出力します。", '\u{03C9}');

single_char!(for_all, "全称記号を出力します。", '\u{2200}');
single_char!(complement, "補集合記号を出力します。", '\u{2201}');
single_char!(partial, "偏微分記号を出力します。", '\u{2202}');
single_char!(exists, "存在記号を出力します。", '\u{2203}');
single_char!(not_exists, "存在しない記号を出力します。", '\u{2204}');
single_char!(emptyset, "空集合記号を出力します。", '\u{2205}');
single_char!(increment, "増分記号を出力します。", '\u{2206}');
single_char!(nabla, "ナブラ記号を出力します。", '\u{2207}');
single_char!(element_of, "要素記号を出力します。", '\u{2208}');
single_char!(not_element_of, "非要素記号を出力します。", '\u{2209}');
single_char!(contains_as_member, "包含記号を出力します。", '\u{220B}');
single_char!(not_contains_as_member, "非包含記号を出力します。", '\u{220C}');
single_char!(end_of_proof, "証明終了記号を出力します。", '\u{220E}');
single_char!(product, "直積記号を出力します。", '\u{220F}');
single_char!(coproduct, "余直積記号を出力します。", '\u{2210}');
single_char!(summation, "総和記号を出力します。", '\u{2211}');
single_char!(minus, "引き算記号を出力します。", '\u{2212}');
single_char!(minus_or_plus, "プラスマイナス記号を出力します。", '\u{2213}');
single_char!(dot_plus, "ドット付きプラス記号を出力します。", '\u{2214}');
single_char!(division, "割り算記号を出力します。", '\u{2215}');
single_char!(square_root, "平方根記号を出力します。", '\u{221A}');
single_char!(proportional_to, "比例記号を出力します。", '\u{221D}');
single_char!(infinity, "無限大記号を出力します。", '\u{221E}');
single_char!(right_angle, "直角記号を出力します。", '\u{221F}');
single_char!(angle, "角度記号を出力します。", '\u{2220}');
single_char!(parallel, "平行記号を出力します。", '\u{2225}');
single_char!(not_parallel, "非平行記号を出力します。", '\u{2226}');
single_char!(logical_and, "論理積記号を出力します。", '\u{2227}');
single_char!(logical_or, "論理和記号を出力します。", '\u{2228}');
single_char!(intersection, "集合の共通部分記号を出力します。", '\u{2229}');
single_char!(union, "集合の和集合記号を出力します。", '\u{222A}');
single_char!(integral, "積分記号を出力します。", '\u{222B}');
single_char!(double_integral, "二重積分記号を出力します。", '\u{222C}');
single_char!(triple_integral, "三重積分記号を出力します。", '\u{222D}');
single_char!(contour_integral, "線積分記号を出力します。", '\u{222E}');
single_char!(surface_integral, "面積分記号を出力します。", '\u{222F}');
single_char!(volume_integral, "体積分記号を出力します。", '\u{2230}');
single_char!(therefore, "ゆえに記号を出力します。", '\u{2234}');
single_char!(because, "なぜならば記号を出力します。", '\u{2235}');
single_char!(reversed_tilde, "反転チルダ記号を出力します。", '\u{223D}');

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{green::GreenNode, parser::parse, syntax::SyntaxKind};

  /// テスト用: ソースからコマンドビューを取得
  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a GreenNode<'a> {
    let cst = parse(source, arena).unwrap();
    // Root > CommandCall
    for child in cst.children {
      if let crate::green::GreenElement::Node(n) = child
        && n.kind == SyntaxKind::CommandCall
      {
        return n;
      }
    }
    panic!("CommandCall ノードが見つかりません");
  }

  #[test]
  fn emits_lower_sigma() {
    let arena = Bump::new();
    let node = get_command_view("\\sigma", &arena);
    let view = CommandView::new(node, "\\sigma");
    match lower_sigma(&view).map(|mut v| v.remove(0)) {
      Ok(InlineNode::Symbol(ch)) => assert_eq!(ch, 'σ'),
      _ => panic!("lower_sigma は正常に Symbol ノードを返すべきです"),
    }
  }

  #[test]
  fn emits_upper_omega() {
    let arena = Bump::new();
    let node = get_command_view("\\Omega", &arena);
    let view = CommandView::new(node, "\\Omega");
    match upper_omega(&view).map(|mut v| v.remove(0)) {
      Ok(InlineNode::Symbol(ch)) => assert_eq!(ch, 'Ω'),
      _ => panic!("upper_omega は正常に Symbol ノードを返すべきです"),
    }
  }

  #[test]
  fn emits_infinity() {
    let arena = Bump::new();
    let node = get_command_view("\\infty", &arena);
    let view = CommandView::new(node, "\\infty");
    match infinity(&view).map(|mut v| v.remove(0)) {
      Ok(InlineNode::Symbol(ch)) => assert_eq!(ch, '∞'),
      _ => panic!("infinity は正常に Symbol ノードを返すべきです"),
    }
  }

  #[test]
  fn rejects_command_with_required_args() {
    let arena = Bump::new();
    let source = "\\alpha{extra}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);
    match lower_alpha(&view) {
      Err(EvalError::ExtraCommandArgument { name, .. }) => {
        assert_eq!(name, "alpha");
      },
      _ => panic!("引数付きコマンドはエラーを返すべきです"),
    }
  }

  #[test]
  fn rejects_command_with_optional_args() {
    let arena = Bump::new();
    let source = "\\omega[opt]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);
    match lower_omega(&view) {
      Err(EvalError::ExtraCommandArgument { name, .. }) => {
        assert_eq!(name, "omega");
      },
      _ => panic!("任意引数付きコマンドはエラーを返すべきです"),
    }
  }
}
