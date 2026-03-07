// Cryochamber Report Components Library
// 视觉化组件库

// 颜色定义
#let primary-color = rgb("#2563eb")    // 深蓝
#let accent-color = rgb("#f59e0b")     // 橙色
#let text-gray = rgb("#6b7280")        // 灰色
#let bg-light = rgb("#f3f4f6")         // 浅灰背景

// 图标函数（使用 emoji）
#let icon(name) = {
  let icons = (
    clock: "⏰",
    check: "✓",
    cross: "✗",
    star: "★",
    rocket: "🚀",
    brain: "🧠",
    calendar: "📅",
    book: "📚",
    code: "💻",
    chart: "📊",
    person: "👤",
    team: "👥",
    lightbulb: "💡",
    target: "🎯",
    trophy: "🏆",
  )
  text(size: 20pt, icons.at(name, default: "•"))
}

// 对比卡片组件
#let comparison-card(before, after) = {
  grid(
    columns: (1fr, 1fr),
    gutter: 1em,
    // Before
    rect(
      fill: rgb("#fee2e2"),
      inset: 1em,
      radius: 8pt,
      [
        #text(weight: "bold", fill: rgb("#dc2626"))[使用前] \
        #before
      ]
    ),
    // After
    rect(
      fill: rgb("#d1fae5"),
      inset: 1em,
      radius: 8pt,
      [
        #text(weight: "bold", fill: rgb("#059669"))[使用后] \
        #after
      ]
    )
  )
}

// 统计数字组件
#let stat-box(number, label) = {
  align(center)[
    #text(size: 36pt, weight: "bold", fill: primary-color)[#number] \
    #text(size: 12pt, fill: text-gray)[#label]
  ]
}

// 引用框组件
#let quote-box(content, author) = {
  box(
    fill: rgb("#dbeafe"),
    inset: 1.5em,
    radius: 8pt,
    width: 100%,
  )[
    #text(size: 13pt, style: "italic")[#content]

    #align(right)[
      #text(size: 11pt, fill: text-gray)[— #author]
    ]
  ]
}

// 故事框组件
#let story-box(content) = {
  box(
    fill: bg-light,
    inset: 1.5em,
    radius: 8pt,
    width: 100%,
  )[
    #text(style: "italic")[#content]
  ]
}
