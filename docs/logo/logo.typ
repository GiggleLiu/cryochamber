// Cryochamber Logo V12: Eclipse with Ring Gap
// Concentric offset circles with a white ring reveal
#import "@preview/cetz:0.4.2": canvas, draw

#set page(width: auto, height: auto, margin: 0.8cm)

#canvas(length: 1cm, {
  import draw: *

  let navy  = rgb("#1a2744")
  let slate = rgb("#2d5a8e")
  let sky   = rgb("#5ba3d9")
  let ice   = rgb("#a8d8ea")
  let frost = rgb("#e0f0f8")

  // Outer ring
  circle((0, 0), radius: 1.8, fill: navy, stroke: none)
  // White gap ring
  circle((0, 0.05), radius: 1.62, fill: white, stroke: none)
  // Inner filled layers
  circle((0, 0.1), radius: 1.48, fill: slate, stroke: none)
  circle((0, 0.25), radius: 1.05, fill: sky, stroke: none)
  circle((0, 0.35), radius: 0.6, fill: ice, stroke: none)
  circle((0, 0.38), radius: 0.25, fill: frost, stroke: none)

  // Wordmark — single line, right side, vertically centered
  content((3.0, 0),
          text(size: 36pt, weight: "bold", fill: navy)[cryochamber],
          anchor: "west")

  // Spacer to widen canvas
  content((12, 0), [])
})
