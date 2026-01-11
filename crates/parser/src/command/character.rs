//! ギリシャ文字出力コマンド群
//!
//! パーサーから呼ばれるギリシャ文字コマンドを提供します。すべての関数は
//! 指定された`Style`で単一文字の`LayoutNode::Text`を生成します。

use crate::{
  evaluator::{EvalError, LayoutNode, Style},
  parser::Command,
};

/// 単一の文字関数を生成するヘルパーマクロ
///
/// このマクロは指定されたUnicode文字を返す関数を生成します。
/// 生成される関数は引数なしでコマンドを受け取り、
/// 指定されたスタイルで単一文字の`LayoutNode::Text`を返します。
///
/// # エラーハンドリング
///
/// - 必須引数（`{}`）が指定された場合は [`EvalError::ExtraCommandArgument`] を返します
/// - 任意引数（`[]`）が指定された場合は [`EvalError::ExtraCommandArgument`] を返します
macro_rules! single_char {
  ($fn_name:ident, $doc:expr, $ch:expr) => {
    #[doc = concat!($doc, "\n\n# エラー\n\n引数が指定されている場合は [`EvalError::ExtraCommandArgument`] を返します。")]
    #[inline]
    pub(super) fn $fn_name(command: Command, style: Style) -> Result<LayoutNode, EvalError> {
      if !command.args.is_empty() || !command.opt_args.is_empty() {
        return Err(EvalError::ExtraCommandArgument(command.name.to_string()));
      }
      return Ok(LayoutNode::Text($ch.to_string(), style));
    }
  };
}

single_char!(upper_alpha, "ギリシャ文字の大文字アルファを出力します。", "\u{0391}");
single_char!(upper_beta, "ギリシャ文字の大文字ベータを出力します。", "\u{0392}");
single_char!(upper_gamma, "ギリシャ文字の大文字ガンマを出力します。", "\u{0393}");
single_char!(upper_delta, "ギリシャ文字の大文字デルタを出力します。", "\u{0394}");
single_char!(upper_epsilon, "ギリシャ文字の大文字イプシロンを出力します。", "\u{0395}");
single_char!(upper_zeta, "ギリシャ文字の大文字ゼータを出力します。", "\u{0396}");
single_char!(upper_eta, "ギリシャ文字の大文字エータを出力します。", "\u{0397}");
single_char!(upper_theta, "ギリシャ文字の大文字シータを出力します。", "\u{0398}");
single_char!(upper_iota, "ギリシャ文字の大文字イオタを出力します。", "\u{0399}");
single_char!(upper_kappa, "ギリシャ文字の大文字カッパを出力します。", "\u{039A}");
single_char!(upper_lambda, "ギリシャ文字の大文字ラムダを出力します。", "\u{039B}");
single_char!(upper_mu, "ギリシャ文字の大文字ミューを出力します。", "\u{039C}");
single_char!(upper_nu, "ギリシャ文字の大文字ニューを出力します。", "\u{039D}");
single_char!(upper_xi, "ギリシャ文字の大文字クシーを出力します。", "\u{039E}");
single_char!(upper_omicron, "ギリシャ文字の大文字オミクロンを出力します。", "\u{039F}");
single_char!(upper_pi, "ギリシャ文字の大文字パイを出力します。", "\u{03A0}");
single_char!(upper_rho, "ギリシャ文字の大文字ローを出力します。", "\u{03A1}");
single_char!(upper_sigma, "ギリシャ文字の大文字シグマを出力します。", "\u{03A3}");
single_char!(upper_tau, "ギリシャ文字の大文字タウを出力します。", "\u{03A4}");
single_char!(upper_upsilon, "ギリシャ文字の大文字ユプシロンを出力します。", "\u{03A5}");
single_char!(upper_phi, "ギリシャ文字の大文字ファイを出力します。", "\u{03A6}");
single_char!(upper_chi, "ギリシャ文字の大文字カイを出力します。", "\u{03A7}");
single_char!(upper_psi, "ギリシャ文字の大文字プサイを出力します。", "\u{03A8}");
single_char!(upper_omega, "ギリシャ文字の大文字オメガを出力します。", "\u{03A9}");
single_char!(lower_alpha, "ギリシャ文字の小文字アルファを出力します。", "\u{03B1}");
single_char!(lower_beta, "ギリシャ文字の小文字ベータを出力します。", "\u{03B2}");
single_char!(lower_gamma, "ギリシャ文字の小文字ガンマを出力します。", "\u{03B3}");
single_char!(lower_delta, "ギリシャ文字の小文字デルタを出力します。", "\u{03B4}");
single_char!(lower_epsilon, "ギリシャ文字の小文字イプシロンを出力します。", "\u{03B5}");
single_char!(var_epsilon, "ギリシャ文字の小文字イプシロン（バリアント）を出力します。", "\u{03F5}");
single_char!(lower_zeta, "ギリシャ文字の小文字ゼータを出力します。", "\u{03B6}");
single_char!(lower_eta, "ギリシャ文字の小文字エータを出力します。", "\u{03B7}");
single_char!(lower_theta, "ギリシャ文字の小文字シータを出力します。", "\u{03B8}");
single_char!(var_theta, "ギリシャ文字の小文字シータ（バリアント）を出力します。", "\u{03D1}");
single_char!(lower_iota, "ギリシャ文字の小文字イオタを出力します。", "\u{03B9}");
single_char!(lower_kappa, "ギリシャ文字の小文字カッパを出力します。", "\u{03BA}");
single_char!(var_kappa, "ギリシャ文字の小文字カッパ（バリアント）を出力します。", "\u{03F0}");
single_char!(lower_lambda, "ギリシャ文字の小文字ラムダを出力します。", "\u{03BB}");
single_char!(lower_mu, "ギリシャ文字の小文字ミューを出力します。", "\u{03BC}");
single_char!(lower_nu, "ギリシャ文字の小文字ニューを出力します。", "\u{03BD}");
single_char!(lower_xi, "ギリシャ文字の小文字クシーを出力します。", "\u{03BE}");
single_char!(lower_omicron, "ギリシャ文字の小文字オミクロンを出力します。", "\u{03BF}");
single_char!(lower_pi, "ギリシャ文字の小文字パイを出力します。", "\u{03C0}");
single_char!(var_pi, "ギリシャ文字の小文字パイ（バリアント）を出力します。", "\u{03D6}");
single_char!(lower_rho, "ギリシャ文字の小文字ローを出力します。", "\u{03C1}");
single_char!(var_rho, "ギリシャ文字の小文字ロー（バリアント）を出力します。", "\u{03F1}");
single_char!(lower_sigma, "ギリシャ文字の小文字シグマを出力します。", "\u{03C3}");
single_char!(final_sigma, "ギリシャ文字の小文字シグマ（語末形）を出力します。", "\u{03C2}");
single_char!(lower_tau, "ギリシャ文字の小文字タウを出力します。", "\u{03C4}");
single_char!(lower_upsilon, "ギリシャ文字の小文字ユプシロンを出力します。", "\u{03C5}");
single_char!(lower_phi, "ギリシャ文字の小文字ファイを出力します。", "\u{03C6}");
single_char!(lower_chi, "ギリシャ文字の小文字カイを出力します。", "\u{03C7}");
single_char!(lower_psi, "ギリシャ文字の小文字プサイを出力します。", "\u{03C8}");
single_char!(lower_omega, "ギリシャ文字の小文字オメガを出力します。", "\u{03C9}");

single_char!(for_all, "全称記号を出力します。", "\u{2200}");
single_char!(complement, "補集合記号を出力します。", "\u{2201}");
single_char!(partial, "偏微分記号を出力します。", "\u{2202}");
single_char!(exists, "存在記号を出力します。", "\u{2203}");
single_char!(not_exists, "存在しない記号を出力します。", "\u{2204}");
single_char!(emptyset, "空集合記号を出力します。", "\u{2205}");
single_char!(increment, "増分記号を出力します。", "\u{2206}");
single_char!(nabla, "ナブラ記号を出力します。", "\u{2207}");
single_char!(element_of, "要素記号を出力します。", "\u{2208}");
single_char!(not_element_of, "非要素記号を出力します。", "\u{2209}");
single_char!(contains_as_member, "包含記号を出力します。", "\u{220B}");
single_char!(not_contains_as_member, "非包含記号を出力します。", "\u{220C}");
single_char!(end_of_proof, "証明終了記号を出力します。", "\u{220E}");
single_char!(product, "直積記号を出力します。", "\u{220F}");
single_char!(coproduct, "余直積記号を出力します。", "\u{2210}");
single_char!(summation, "総和記号を出力します。", "\u{2211}");
single_char!(minus, "引き算記号を出力します。", "\u{2212}");
single_char!(minus_or_plus, "プラスマイナス記号を出力します。", "\u{2213}");
single_char!(dot_plus, "ドット付きプラス記号を出力します。", "\u{2214}");
single_char!(division, "割り算記号を出力します。", "\u{2215}");
single_char!(square_root, "平方根記号を出力します。", "\u{221A}");
single_char!(proportional_to, "比例記号を出力します。", "\u{221D}");
single_char!(infinity, "無限大記号を出力します。", "\u{221E}");
single_char!(right_angle, "直角記号を出力します。", "\u{221F}");
single_char!(angle, "角度記号を出力します。", "\u{2220}");
single_char!(parallel, "平行記号を出力します。", "\u{2225}");
single_char!(not_parallel, "非平行記号を出力します。", "\u{2226}");
single_char!(logical_and, "論理積記号を出力します。", "\u{2227}");
single_char!(logical_or, "論理和記号を出力します。", "\u{2228}");
single_char!(intersection, "集合の共通部分記号を出力します。", "\u{2229}");
single_char!(union, "集合の和集合記号を出力します。", "\u{222A}");
single_char!(integral, "積分記号を出力します。", "\u{222B}");
single_char!(double_integral, "二重積分記号を出力します。", "\u{222C}");
single_char!(triple_integral, "三重積分記号を出力します。", "\u{222D}");
single_char!(contour_integral, "線積分記号を出力します。", "\u{222E}");
single_char!(surface_integral, "面積分記号を出力します。", "\u{222F}");
single_char!(volume_integral, "体積分記号を出力します。", "\u{2230}");
single_char!(therefore, "ゆえに記号を出力します。", "\u{2234}");
single_char!(because, "なぜならば記号を出力します。", "\u{2235}");
single_char!(reversed_tilde, "反転チルダ記号を出力します。", "\u{223D}");

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{evaluator::FontStyle, parser::Command};

  /// テスト用のスタイルを生成
  fn test_style() -> Style {
    return Style {
      font_size: 12.0,
      font_type: FontStyle::Serif,
    };
  }

  /// テスト用のコマンドを生成
  fn test_command(name: &'static str) -> Command<'static> {
    return Command {
      name,
      args: vec![],
      opt_args: vec![],
    };
  }

  /// 引数を持つテストコマンドを生成
  fn test_command_with_args(name: &'static str) -> Command<'static> {
    return Command {
      name,
      args: vec![vec![]],
      opt_args: vec![],
    };
  }

  #[test]
  fn emits_lower_sigma() {
    let style = test_style();
    let command = test_command("sigma");
    match lower_sigma(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "σ");
        assert_eq!(st, style);
      },
      _ => panic!("lower_sigma は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_upper_omega() {
    let style = test_style();
    let command = test_command("Omega");
    match upper_omega(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "Ω");
        assert_eq!(st, style);
      },
      _ => panic!("upper_omega は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_upper_alpha() {
    let style = test_style();
    let command = test_command("Alpha");
    match upper_alpha(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "Α");
        assert_eq!(st, style);
      },
      _ => panic!("upper_alpha は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_lower_alpha() {
    let style = test_style();
    let command = test_command("alpha");
    match lower_alpha(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "α");
        assert_eq!(st, style);
      },
      _ => panic!("lower_alpha は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_final_sigma() {
    let style = test_style();
    let command = test_command("final_sigma");
    match final_sigma(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "ς");
        assert_eq!(st, style);
      },
      _ => panic!("final_sigma は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_infinity() {
    let style = test_style();
    let command = test_command("infty");
    match infinity(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "∞");
        assert_eq!(st, style);
      },
      _ => panic!("infinity は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_integral() {
    let style = test_style();
    let command = test_command("int");
    match integral(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "∫");
        assert_eq!(st, style);
      },
      _ => panic!("integral は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_summation() {
    let style = test_style();
    let command = test_command("sum");
    match summation(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "∑");
        assert_eq!(st, style);
      },
      _ => panic!("summation は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_partial() {
    let style = test_style();
    let command = test_command("partial");
    match partial(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "∂");
        assert_eq!(st, style);
      },
      _ => panic!("partial は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_nabla() {
    let style = test_style();
    let command = test_command("nabla");
    match nabla(command, style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "∇");
        assert_eq!(st, style);
      },
      _ => panic!("nabla は正常に Text ノードを返すべきです"),
    }
    return;
  }

  #[test]
  fn rejects_command_with_required_args() {
    let style = test_style();
    let command = test_command_with_args("alpha");
    match lower_alpha(command, style) {
      Err(EvalError::ExtraCommandArgument(name)) => {
        assert_eq!(name, "alpha");
      },
      _ => panic!("引数付きコマンドはエラーを返すべきです"),
    }
    return;
  }

  #[test]
  fn rejects_command_with_optional_args() {
    let style = test_style();
    let mut command = test_command("omega");
    command.opt_args = vec![vec![]];
    match lower_omega(command, style) {
      Err(EvalError::ExtraCommandArgument(name)) => {
        assert_eq!(name, "omega");
      },
      _ => panic!("任意引数付きコマンドはエラーを返すべきです"),
    }
    return;
  }

  #[test]
  fn emits_various_math_symbols() {
    let style = test_style();

    let command_for_all = test_command("for_all");
    match for_all(command_for_all, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∀"),
      _ => panic!("for_all は正常に Text ノードを返すべきです"),
    }

    let command_exists = test_command("exists");
    match exists(command_exists, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∃"),
      _ => panic!("exists は正常に Text ノードを返すべきです"),
    }

    let command_emptyset = test_command("emptyset");
    match emptyset(command_emptyset, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∅"),
      _ => panic!("emptyset は正常に Text ノードを返すべきです"),
    }

    let command_element = test_command("element_of");
    match element_of(command_element, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∈"),
      _ => panic!("element_of は正常に Text ノードを返すべきです"),
    }

    let command_union = test_command("union");
    match union(command_union, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∪"),
      _ => panic!("union は正常に Text ノードを返すべきです"),
    }

    let command_intersection = test_command("intersection");
    match intersection(command_intersection, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "∩"),
      _ => panic!("intersection は正常に Text ノードを返すべきです"),
    }

    return;
  }

  #[test]
  fn emits_greek_variants() {
    let style = test_style();

    let command_var_eps = test_command("var_epsilon");
    match var_epsilon(command_var_eps, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "ϵ"),
      _ => panic!("var_epsilon は正常に Text ノードを返すべきです"),
    }

    let command_var_th = test_command("var_theta");
    match var_theta(command_var_th, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "ϑ"),
      _ => panic!("var_theta は正常に Text ノードを返すべきです"),
    }

    let command_var_kap = test_command("var_kappa");
    match var_kappa(command_var_kap, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "ϰ"),
      _ => panic!("var_kappa は正常に Text ノードを返すべきです"),
    }

    let command_var_pi_var = test_command("var_pi");
    match var_pi(command_var_pi_var, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "ϖ"),
      _ => panic!("var_pi は正常に Text ノードを返すべきです"),
    }

    let command_var_rho_var = test_command("var_rho");
    match var_rho(command_var_rho_var, style) {
      Ok(LayoutNode::Text(s, _)) => assert_eq!(s, "ϱ"),
      _ => panic!("var_rho は正常に Text ノードを返すべきです"),
    }

    return;
  }

  #[test]
  fn preserves_style_through_emission() {
    let custom_style = Style {
      font_size: 24.5,
      font_type: FontStyle::Math,
    };
    let command = test_command("beta");
    match lower_beta(command, custom_style) {
      Ok(LayoutNode::Text(s, st)) => {
        assert_eq!(s, "β");
        assert!((st.font_size - 24.5).abs() < f32::EPSILON);
        assert_eq!(st.font_type, FontStyle::Math);
      },
      _ => panic!("スタイルが保持されるべきです"),
    }
    return;
  }
}
