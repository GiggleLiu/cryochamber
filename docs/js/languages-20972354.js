// Language switcher for the Cryochamber book (English <-> 中文).
// The Chinese book is built into /zh/ (see Makefile `book` target and
// .github/workflows/docs.yml). Each page carries a link to its counterpart
// in the other language: the path is identical except for the optional
// leading /zh/ segment, so switching is a pure path rewrite.
(function () {
  function addLangSwitch() {
    var path = window.location.pathname;
    var m = path.match(/^(.*\/)([^/]+\.html)$/); // split dir + page file
    var dir = m ? m[1] : (path.endsWith('/') ? path : path + '/');
    var file = m ? '/' + m[2] : '';
    var isZh = /\/zh\/$/.test(dir);
    var target;
    if (isZh) {
      var base = dir.slice(0, -4); // strip trailing '/zh/'
      target = base + file || '/';
    } else {
      target = dir + 'zh/' + (file ? file.slice(1) : '');
    }
    var a = document.createElement('a');
    a.href = target;
    a.className = 'lang-switch';
    a.textContent = isZh ? 'English' : '中文';
    a.setAttribute('aria-label', isZh ? 'Switch to English' : '切换到中文');
    var bar = document.querySelector('.right-buttons');
    if (!bar) return;
    bar.insertBefore(a, bar.firstChild);
  }
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', addLangSwitch);
  } else {
    addLangSwitch();
  }
})();
