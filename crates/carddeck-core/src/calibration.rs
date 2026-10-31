//! Built-in calibration card for the first test print (orientation, edge clipping, resin black).

use serde_json::{json, Value};

use crate::Card;

fn rect(x: f64, y: f64, w: f64, h: f64, fill: &str) -> Value {
    json!({"className":"Rect","name":"Rect","x":x,"y":y,"width":w,"height":h,"fill":fill,"fillPriority":"color","strokeWidth":0})
}

fn text(s: &str, x: f64, y: f64, w: f64, h: f64, size: f64, fill: &str, align: &str) -> Value {
    json!({"className":"Text","name":"Text","text":s,"x":x,"y":y,"width":w,"height":h,"fontSize":size,
           "fontFamily":"Helvetica Neue","fontStyle":"normal bold","fill":fill,"align":align,"verticalAlign":"middle","wrap":"word"})
}

fn side(label: &str, accent: &str) -> Value {
    let mut border = rect(5.0, 5.0, 1003.0, 632.0, "");
    border["stroke"] = json!("#000000");
    border["strokeWidth"] = json!(4);
    border["fillType"] = json!("none");
    let arrow = json!({"className":"Line","name":"Triangle","x":506.5,"y":30.0,"closed":true,"points":[0,0,-50,70,50,70],"fill":accent,"fillPriority":"color"});
    let mut grad = rect(130.0, 530.0, 753.0, 70.0, "#ffffff");
    grad["fillPriority"] = json!("linear-gradient");
    grad["fillLinearGradientColorStops"] = json!([0, "#ff0000", 0.33, "#ffff00", 0.66, "#00a0ff", 1, "#000000"]);
    grad["fillLinearGradientStartPointX"] = json!(0);
    grad["fillLinearGradientStartPointY"] = json!(0);
    grad["fillLinearGradientEndPointX"] = json!(753);
    grad["fillLinearGradientEndPointY"] = json!(0);
    json!({
        "background": {"width":1013,"height":642,"cornerRadius":20,"fill":"#ffffff","fillType":"colour","fillPriority":"color"},
        "elements": [
            border,
            rect(20.0, 20.0, 90.0, 90.0, "#ff0000"),
            rect(903.0, 20.0, 90.0, 90.0, "#00a000"),
            rect(903.0, 532.0, 90.0, 90.0, "#0000ff"),
            rect(20.0, 532.0, 90.0, 90.0, "#000000"),
            arrow,
            text("TOP", 356.5, 105.0, 300.0, 50.0, 40.0, "#000000", "center"),
            text(label, 120.0, 210.0, 773.0, 120.0, 100.0, accent, "center"),
            text("Pure black #000000 text (resin K)", 120.0, 350.0, 773.0, 44.0, 36.0, "#000000", "center"),
            text("Near black #101010 text (composite)", 120.0, 394.0, 773.0, 44.0, 36.0, "#101010", "center"),
            text("top-left = red   bottom-left = black", 120.0, 470.0, 773.0, 40.0, 30.0, "#444444", "center"),
            grad,
        ]
    })
}

/// A synthetic double-sided CR80 card used to calibrate orientation and see edge behaviour.
pub fn calibration_card() -> Card {
    let doc = json!({
        "about": {"hubVersion":"calibration","type":{"name":"CR80","widthPixels":1013,"heightPixels":642,"CardSize":0},
                  "doubleSided": true, "portrait": false, "darkCard": false},
        "front": side("FRONT", "#d02020"),
        "back": side("BACK", "#1060d0"),
    });
    let mut card = Card::parse(&serde_json::to_vec(&doc).unwrap()).expect("calibration card is valid");
    // The calibration card must show its exact colours: no preview correction.
    card.adjustments = crate::colour::Adjustments::identity();
    card
}
