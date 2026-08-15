export const codeBlock =
  '<div class="codehilite" data-code-language="Python"><pre><span></span><code><span class="k">def</span> <span class="nf">f</span><span class="p">():</span>\n    <span class="k">pass</span>\n</code></pre></div>'

export const katexMath =
  '<p><span class="katex"><span class="katex-html" aria-hidden="true"><span class="base"><span class="mord mathnormal">x</span></span></span></span></p>'

export const uploadLink =
  '<p><a href="/user_uploads/2/ab/report.pdf">report.pdf</a></p>'

export const inlineImage =
  '<div class="message_inline_image"><a href="/user_uploads/2/ab/plot.png"><img src="/user_uploads/2/ab/plot.png"></a></div>'

export const externalLink =
  '<p><a href="https://arxiv.org/abs/2401.00001">paper</a></p>'

export const hostileScript = '<p>hi</p><script>window.hacked = true</script>'

export const hostileImgHandler = '<img src="x" onerror="window.hacked = true">'

export const hostileJsHref = '<a href="javascript:window.hacked=1">click</a>'

// Realistic KaTeX display math output: .katex-mathml carries the MathML
// fallback (with a raw-TeX <annotation>) that must be dropped; .katex-html
// carries the visible layout, whose positioning lives in inline styles.
export const katexDisplayMath =
  '<span class="katex-display"><span class="katex">' +
  '<span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML">' +
  '<semantics><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow>' +
  '<annotation encoding="application/x-tex">x^2</annotation></semantics></math></span>' +
  '<span class="katex-html" aria-hidden="true"><span class="base">' +
  '<span class="strut" style="height:0.8141em;"></span>' +
  '<span class="mord"><span class="mord mathnormal">x</span>' +
  '<span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;">' +
  '<span style="top:-3.063em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span>' +
  '<span class="sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span>' +
  '</span></span></span></span></span></span></span></span></span></span>'

// KaTeX sqrt stretchy-delimiter SVG (hide-tail span wrapping an svg/path).
export const katexSvgSqrt =
  '<span class="hide-tail" style="min-width:0.853em;height:1.08em;">' +
  '<svg xmlns="http://www.w3.org/2000/svg" width="400em" height="1.08em" ' +
  'viewBox="0 0 400000 1080" preserveAspectRatio="xMinYMin slice">' +
  '<path d="M95,702c-2.7,0,-7.17,-2.7,-13.5,-8c-5.8,-5.3,-9.5,-10,-9.5,-14"/>' +
  '</svg></span>'

export const tableMarkup =
  '<table><thead><tr><th>name</th><th>score</th></tr></thead>' +
  '<tbody><tr><td>a</td><td>1</td></tr><tr><td>b</td><td>2</td></tr></tbody></table>'

export const emojiThumbsUp =
  '<p><span class="emoji emoji-1f44d" title="thumbs up">:thumbs_up:</span></p>'

export const emojiFlagCn = '<p><span class="emoji emoji-1f1e8-1f1f3">:cn:</span></p>'

export const emojiImg =
  '<p><img class="emoji" alt="🎉" src="/static/generated/emoji/tada.png"></p>'

export const userMention =
  '<p>ping <span class="user-mention" data-user-id="42" title="@Alice Doe">@Alice Doe</span></p>'

export const userGroupMention =
  '<p><span class="user-group-mention" data-user-group-id="1">@everyone</span></p>'
