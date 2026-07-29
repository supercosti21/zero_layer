/* ==========================================================================
   Zero Layer — site behaviour
   Theme, language, navigation, tabs, copy buttons, terminal demo, reveals.
   ========================================================================== */

(function () {
    'use strict';

    var SUPPORTED = ['en', 'it', 'fr', 'es', 'de'];
    var DEFAULT_LANG = 'en';
    var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    function $(sel, root) { return (root || document).querySelector(sel); }
    function $$(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }
    function store(key, val) { try { if (val === undefined) return localStorage.getItem(key); localStorage.setItem(key, val); } catch (e) { return null; } }

    /* ---------------------------------------------------------------- Theme */

    function setTheme(theme) {
        document.documentElement.setAttribute('data-theme', theme);
        var meta = $('#theme-color-meta');
        if (meta) meta.setAttribute('content', theme === 'light' ? '#fbfbfd' : '#07070b');
        var btn = $('#theme-toggle');
        if (btn) btn.setAttribute('aria-pressed', theme === 'light' ? 'true' : 'false');
        store('zl-theme', theme);
    }

    function initTheme() {
        var btn = $('#theme-toggle');
        if (!btn) return;
        btn.addEventListener('click', function () {
            var current = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
            setTheme(current === 'light' ? 'dark' : 'light');
        });
    }

    /* ------------------------------------------------------------- Language */

    function translate(lang) {
        var dict = window.ZL_I18N[lang] || window.ZL_I18N[DEFAULT_LANG];
        var fallback = window.ZL_I18N[DEFAULT_LANG];

        $$('[data-i18n]').forEach(function (el) {
            var key = el.getAttribute('data-i18n');
            var value = dict[key] !== undefined ? dict[key] : fallback[key];
            if (value === undefined) return;
            // These keys carry inline markup (<strong>, <code>); the rest is plain text.
            if (/_(p1|p2|html)$/.test(key) || /^a[0-9]+$/.test(key) || key === 'rel_notice') el.innerHTML = value;
            else el.textContent = value;
        });

        $$('[data-i18n-attr]').forEach(function (el) {
            // Format: "aria-label:nav_menu" — attribute name, then key.
            el.getAttribute('data-i18n-attr').split(',').forEach(function (pair) {
                var parts = pair.split(':');
                var attr = parts[0].trim();
                var key = parts[1].trim();
                var value = dict[key] !== undefined ? dict[key] : fallback[key];
                if (value !== undefined) el.setAttribute(attr, value);
            });
        });

        document.documentElement.lang = lang;
        var desc = $('meta[name="description"]');
        if (desc && dict.meta_desc) desc.setAttribute('content', dict.meta_desc);

        var select = $('#lang-select');
        if (select) select.value = lang;
        store('zl-lang', lang);
        resetCopyLabels();
    }

    function initLang() {
        var lang = store('zl-lang');
        var param = new URLSearchParams(window.location.search).get('lang');
        if (param && SUPPORTED.indexOf(param) !== -1) lang = param;
        if (!lang || SUPPORTED.indexOf(lang) === -1) {
            var browser = (navigator.language || DEFAULT_LANG).slice(0, 2).toLowerCase();
            lang = SUPPORTED.indexOf(browser) !== -1 ? browser : DEFAULT_LANG;
        }
        translate(lang);

        var select = $('#lang-select');
        if (select) select.addEventListener('change', function (e) { translate(e.target.value); });
    }

    /* ------------------------------------------------------------------ Nav */

    function initNav() {
        var nav = $('#nav');
        var progress = $('#nav-progress');
        var links = $('#nav-links');
        var menuBtn = $('#menu-btn');

        function onScroll() {
            var y = window.scrollY;
            if (nav) nav.classList.toggle('is-scrolled', y > 8);
            if (progress) {
                var max = document.documentElement.scrollHeight - window.innerHeight;
                progress.style.width = (max > 0 ? (y / max) * 100 : 0) + '%';
            }
        }
        window.addEventListener('scroll', onScroll, { passive: true });
        onScroll();

        if (menuBtn && links) {
            menuBtn.addEventListener('click', function () {
                var open = links.classList.toggle('is-open');
                menuBtn.setAttribute('aria-expanded', open ? 'true' : 'false');
            });
            $$('a', links).forEach(function (a) {
                a.addEventListener('click', function () {
                    links.classList.remove('is-open');
                    menuBtn.setAttribute('aria-expanded', 'false');
                });
            });
            document.addEventListener('keydown', function (e) {
                if (e.key === 'Escape' && links.classList.contains('is-open')) {
                    links.classList.remove('is-open');
                    menuBtn.setAttribute('aria-expanded', 'false');
                    menuBtn.focus();
                }
            });
        }
    }

    /* ----------------------------------------------------------------- Tabs */

    function initTabs() {
        $$('[data-tabs]').forEach(function (group) {
            var tabs = $$('[role="tab"]', group);
            var panels = $$('[role="tabpanel"]', group);

            function select(index) {
                tabs.forEach(function (tab, i) {
                    var active = i === index;
                    tab.setAttribute('aria-selected', active ? 'true' : 'false');
                    tab.tabIndex = active ? 0 : -1;
                });
                panels.forEach(function (panel, i) { panel.hidden = i !== index; });
            }

            tabs.forEach(function (tab, i) {
                tab.addEventListener('click', function () { select(i); });
                tab.addEventListener('keydown', function (e) {
                    var next = null;
                    if (e.key === 'ArrowRight') next = (i + 1) % tabs.length;
                    if (e.key === 'ArrowLeft') next = (i - 1 + tabs.length) % tabs.length;
                    if (e.key === 'Home') next = 0;
                    if (e.key === 'End') next = tabs.length - 1;
                    if (next !== null) { e.preventDefault(); select(next); tabs[next].focus(); }
                });
            });
            select(0);
        });
    }

    /* --------------------------------------------------------- Copy buttons */

    function resetCopyLabels() {
        $$('.copy-btn').forEach(function (btn) {
            if (btn.classList.contains('is-copied')) return;
            var label = $('[data-i18n="copy"]', btn);
            if (label) {
                var lang = document.documentElement.lang;
                var dict = window.ZL_I18N[lang] || window.ZL_I18N[DEFAULT_LANG];
                label.textContent = dict.copy;
            }
        });
    }

    function initCopy() {
        $$('.copy-btn').forEach(function (btn) {
            btn.addEventListener('click', function () {
                var targetSel = btn.getAttribute('data-copy-target');
                var text = targetSel ? ($(targetSel) || {}).innerText : btn.getAttribute('data-copy');
                if (!text) return;
                text = text.replace(/^\s*\$\s?/gm, '').trim();

                var done = function () {
                    var lang = document.documentElement.lang;
                    var dict = window.ZL_I18N[lang] || window.ZL_I18N[DEFAULT_LANG];
                    var label = $('[data-i18n="copy"]', btn);
                    btn.classList.add('is-copied');
                    if (label) label.textContent = dict.copied;
                    setTimeout(function () {
                        btn.classList.remove('is-copied');
                        if (label) label.textContent = dict.copy;
                    }, 1800);
                };

                if (navigator.clipboard && navigator.clipboard.writeText) {
                    navigator.clipboard.writeText(text).then(done).catch(function () {});
                } else {
                    var ta = document.createElement('textarea');
                    ta.value = text;
                    ta.style.position = 'fixed';
                    ta.style.opacity = '0';
                    document.body.appendChild(ta);
                    ta.select();
                    try { document.execCommand('copy'); done(); } catch (e) {}
                    document.body.removeChild(ta);
                }
            });
        });
    }

    /* -------------------------------------------------------- Terminal demo */

    var TERMINAL_SCRIPT = [
        { t: 'cmd',     text: 'zl install ripgrep' },
        { t: 'comment', text: '# no --from: every enabled source is queried' },
        { t: 'dim',     text: '  ? Found in 4 sources — pacman, apt, nix, github' },
        { t: 'ok',      text: '  ✓ Resolved 3 dependencies, no conflicts' },
        { t: 'dim',     text: '  [1/4] Downloading   ████████████████  2.4 MB/s' },
        { t: 'ok',      text: '  [2/4] Verified SHA256 + GPG signature' },
        { t: 'ok',      text: '  [3/4] Patched 1 ELF  interpreter + RUNPATH' },
        { t: 'ok',      text: '  [4/4] Installed ripgrep 14.1.1  [8 files, 5.1 MB]' },
        { t: 'blank',   text: '' },
        { t: 'cmd',     text: 'rg --version' },
        { t: 'dim',     text: 'ripgrep 14.1.1' },
        { t: 'comment', text: '# a native binary — nothing wrapping it' }
    ];

    function renderLine(entry) {
        var div = document.createElement('div');
        div.className = 'line';
        if (entry.t === 'blank') { div.innerHTML = '&nbsp;'; return div; }
        if (entry.t === 'cmd') {
            div.innerHTML = '<span class="prompt">$</span> ' + escapeHtml(entry.text);
        } else {
            div.className = 'line ' + entry.t;
            div.textContent = entry.text;
        }
        return div;
    }

    function escapeHtml(s) {
        return s.replace(/[&<>]/g, function (c) { return ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]; });
    }

    function initTerminal() {
        var body = $('#terminal-body');
        if (!body) return;

        if (reduceMotion) {
            TERMINAL_SCRIPT.forEach(function (entry) { body.appendChild(renderLine(entry)); });
            return;
        }

        var started = false;
        var observer = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting || started) return;
                started = true;
                observer.disconnect();
                play();
            });
        }, { threshold: 0.25 });
        observer.observe(body);

        function play() {
            var i = 0;
            var cursor = document.createElement('span');
            cursor.className = 'cursor';

            function step() {
                if (i >= TERMINAL_SCRIPT.length) {
                    body.appendChild(cursor);
                    return;
                }
                var entry = TERMINAL_SCRIPT[i++];
                if (cursor.parentNode) cursor.parentNode.removeChild(cursor);
                body.appendChild(renderLine(entry));
                body.appendChild(cursor);
                body.scrollTop = body.scrollHeight;
                setTimeout(step, entry.t === 'cmd' ? 620 : entry.t === 'blank' ? 260 : 400);
            }
            step();
        }
    }

    /* ---------------------------------------------------------- Stat counts */

    function initCounters() {
        var nums = $$('[data-count]');
        if (!nums.length) return;

        if (reduceMotion) {
            nums.forEach(function (el) { el.textContent = el.getAttribute('data-count') + (el.getAttribute('data-suffix') || ''); });
            return;
        }

        var observer = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting) return;
                var el = entry.target;
                observer.unobserve(el);
                var target = parseInt(el.getAttribute('data-count'), 10);
                var suffix = el.getAttribute('data-suffix') || '';
                var start = performance.now();
                var duration = 1100;

                function tick(now) {
                    var p = Math.min((now - start) / duration, 1);
                    var eased = 1 - Math.pow(1 - p, 3);
                    el.textContent = Math.round(target * eased) + suffix;
                    if (p < 1) requestAnimationFrame(tick);
                }
                requestAnimationFrame(tick);
            });
        }, { threshold: 0.5 });

        nums.forEach(function (el) { observer.observe(el); });
    }

    /* -------------------------------------------------------------- Reveals */

    function initReveals() {
        var items = $$('.reveal, .reveal-stagger');
        if (!items.length) return;
        if (reduceMotion) { items.forEach(function (el) { el.classList.add('is-visible'); }); return; }

        var observer = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting) return;
                entry.target.classList.add('is-visible');
                observer.unobserve(entry.target);
            });
        }, { threshold: 0.12, rootMargin: '0px 0px -48px 0px' });

        items.forEach(function (el) { observer.observe(el); });
    }

    /* ----------------------------------------------------------------- Year */

    function initYear() {
        var el = $('#year');
        if (el) el.textContent = String(new Date().getFullYear());
    }

    /* ------------------------------------------------------------------ Boot */

    function boot() {
        initTheme();
        initLang();
        initNav();
        initTabs();
        initCopy();
        initTerminal();
        initCounters();
        initReveals();
        initYear();
    }

    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
    else boot();
})();
