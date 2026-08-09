// Language switcher for the Cryochamber book (English <-> 中文).
// The Chinese book is built into book/zh/ (see Makefile `book` target and
// .github/workflows/docs.yml). Each page carries a link to its counterpart
// in the other language.
//
// Links are *relative*, computed from mdbook's `path_to_root` (the `../`
// chain from the current page up to the current book's root) and the
// `<html lang>` attribute (mdbook sets `lang="zh"` in the zh build). This
// works on any base path: GitHub Pages (/cryochamber/), a local static
// server (/), or file:// — no absolute paths, so no 404s.
(function () {
  // mdbook writes `const path_to_root = "../"` inside its own inline script
  // (not on window), but every page also links the sidebar script as
  // `<path_to_root>toc-<hash>.js` — parse the prefix from that src.
  function getPathToRoot() {
    var scripts = document.querySelectorAll('script[src]');
    for (var i = 0; i < scripts.length; i++) {
      var src = scripts[i].getAttribute('src') || '';
      var m = src.match(/^(.*)toc-[^/]+\.js$/);
      if (m) return m[1];
    }
    // Fallback: the print button is `<path_to_root>print.html`.
    var printLink = document.querySelector('a[href$="print.html"]');
    if (printLink) return (printLink.getAttribute('href') || '').replace(/print\.html$/, '');
    return '';
  }

  function countUps(pathToRoot) {
    var n = 0;
    while (pathToRoot.indexOf('../') === 0) {
      n += 1;
      pathToRoot = pathToRoot.slice(3);
    }
    return n;
  }

  function addLangSwitch() {
    var ups = countUps(getPathToRoot());
    var isZh = (document.documentElement.getAttribute('lang') || '').toLowerCase().indexOf('zh') === 0;

    var path = window.location.pathname;
    var trailing = path.charAt(path.length - 1) === '/';
    var segs = path.split('/').filter(Boolean);
    var file = trailing ? '' : (segs.pop() || '');
    // A non-'.html' tail is a directory served without its trailing slash
    // (browsers usually redirect, but be safe): treat it as a directory.
    if (file && file.indexOf('.html') === -1) {
      segs.push(file);
      file = '';
    }
    // The current page's directory relative to the current book's root is
    // the last `ups` segments of the path (path_to_root counts exactly that
    // deep; the base path is all the segments before those).
    var dirSegs = ups > 0 ? segs.slice(-ups) : [];

    // From the current directory, climb up to the book root, then into the
    // other book's mount: zh pages live one level deeper (book root -> /zh/).
    var up = '../'.repeat(ups + (isZh ? 1 : 0));
    var dir = dirSegs.join('/');
    var target = up + (isZh ? '' : 'zh/') + (dir ? dir + '/' : '') + file;

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
