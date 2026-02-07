use font::shaper::HarfRustShapers;

use crate::evaluator::LayoutNode;
pub fn layout_engine(layout_nodes: Vec<LayoutNode>, _shapers: HarfRustShapers) {
  for node in layout_nodes {
    println!("{node:?}");
  }
}
