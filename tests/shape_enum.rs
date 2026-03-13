use mmdflux::Shape;

#[test]
fn shape_new_variants_exist() {
    // Core shapes
    let _ = Shape::Rectangle;
    let _ = Shape::Round;
    let _ = Shape::Diamond;
    let _ = Shape::Stadium;
    let _ = Shape::Subroutine;
    let _ = Shape::Cylinder;

    // New shapes
    let _ = Shape::TextBlock;
    let _ = Shape::ForkJoin;
    let _ = Shape::Document;
    let _ = Shape::Documents;
    let _ = Shape::TaggedDocument;
    let _ = Shape::Card;
    let _ = Shape::TaggedRect;
    let _ = Shape::SmallCircle;
    let _ = Shape::FramedCircle;
    let _ = Shape::CrossedCircle;
    let _ = Shape::Hexagon;
    let _ = Shape::Trapezoid;
    let _ = Shape::InvTrapezoid;
    let _ = Shape::Parallelogram;
    let _ = Shape::InvParallelogram;
    let _ = Shape::ManualInput;
    let _ = Shape::Asymmetric;
    let _ = Shape::Circle;
    let _ = Shape::DoubleCircle;
}

#[test]
fn shape_default_is_rectangle() {
    assert_eq!(Shape::default(), Shape::Rectangle);
}

#[test]
fn shape_is_copy_and_eq() {
    let s1 = Shape::Diamond;
    let s2 = s1; // Copy
    assert_eq!(s1, s2);
}
