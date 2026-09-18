const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

test('native raster mode initializes tiles and markers without MapLibre or WebGL', () => {
    const elements = [];
    function element(tag) {
        const node = { tagName: tag, style: {}, children: [], clientWidth: 800, clientHeight: 600,
            appendChild(child) { this.children.push(child); },
            setAttribute() {}, addEventListener() {}, removeEventListener() {},
            getBoundingClientRect() { return { width: 800, height: 600 }; },
            classList: { add() {}, remove() {}, toggle() {} },
        };
        elements.push(node);
        return node;
    }
    const container = element('div');
    const document = { getElementById(id) { return id === 'ground-map' ? container : null; },
        createElement: element, head: element('head'), body: element('body'),
        documentElement: element('html'), addEventListener() {}, querySelector() { return null; },
    };
    const window = { __gs26_prefer_raster_map: true,
        localStorage: { getItem() { return null; }, setItem() {}, removeItem() {} },
        addEventListener() {}, dispatchEvent() {}, matchMedia() { return { matches: false }; },
        location: { protocol: 'dioxus:' },
    };
    const context = vm.createContext({ window, document, navigator: {}, console,
        setTimeout() { return 1; }, clearTimeout() {}, setInterval() { return 1; }, clearInterval() {},
        requestAnimationFrame() { return 1; }, cancelAnimationFrame() {},
        CustomEvent: class {}, URL, performance: { now: () => 0 },
    });
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../static/ground_map.js'), 'utf8'), context);
    // Call the implementation directly so its public error guard cannot hide failure.
    vm.runInContext('initGroundMap("gs26://local/tiles/{z}/{x}/{y}.jpg", 42, -78, 10, 15, "Rocket")', context);
    assert.equal(window.__gs26_map_fallback_mode, 'raster');
    const images = elements.filter(node => node.tagName === 'img');
    assert.ok(images.length > 0);
    assert.ok(images.every(image => /^gs26:\/\/local\/tiles\/10\//.test(image.src)));
    vm.runInContext('applyGroundMapMarkers(42, -78, 42.001, -78.001)', context);
    assert.ok(elements.some(node => node.textContent === '🚀'));
    assert.ok(elements.some(node => node.textContent === '🧍'));
});

test('tab navigation hides overlay scrollbars without disabling scrolling', () => {
    const source = fs.readFileSync(path.join(__dirname, '../src/telemetry_dashboard/dashboard_component.rs'), 'utf8');
    assert.match(source, /gs26-tab-nav[^\n]+overflow-x:auto[^\n]+scrollbar-width:none/);
    assert.match(source, /gs26-tab-nav::-webkit-scrollbar[^\n]+display:none/);
});
