//! Sequence-diagram rendering.

pub mod text;

use crate::render::text::CharSet;
use crate::timeline::sequence::layout::SequenceLayout;

pub(crate) fn render(layout: &SequenceLayout, charset: &CharSet) -> String {
    text::render(layout, charset)
}
