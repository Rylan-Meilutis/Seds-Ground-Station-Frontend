const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

test('native raster mode initializes tiles and markers without MapLibre or WebGL', () => {
    const elements = [];
    function element(tag) {
        const node = { tagName: tag, style: {}, children: [], listeners: {}, clientWidth: 800, clientHeight: 600,
            appendChild(child) { this.children.push(child); child.parentNode=this; },
            get lastChild() {return this.children.at(-1);},
            remove() { if(this.parentNode)this.parentNode.children=this.parentNode.children.filter(n=>n!==this); },
            setAttribute() {}, addEventListener(name,fn) {this.listeners[name]=fn;}, removeEventListener() {},
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
    assert.equal(elements.filter(node=>node.tagName==='img').length, images.length,
        'telemetry updates must reuse decoded tile images');
    const before=vm.runInContext('rasterFallbackCenterLon',context);
    container.listeners.pointerdown({clientX:100,clientY:100,button:0,pointerId:1});
    container.listeners.pointermove({clientX:180,clientY:100,pointerId:1});
    container.listeners.pointerup({pointerId:1});
    assert.notEqual(vm.runInContext('rasterFallbackCenterLon',context),before);
    container.listeners.wheel({deltaY:-100,preventDefault(){}});
    assert.equal(vm.runInContext('rasterFallbackZoom',context),11);
    container.listeners.keydown({key:'-',preventDefault(){}});
    assert.equal(vm.runInContext('rasterFallbackZoom',context),10);
    assert.equal(elements.filter(n=>n.tagName==='button').length,2,'controls are not duplicated');
});

test('tab navigation hides overlay scrollbars without disabling scrolling', () => {
    const source = fs.readFileSync(path.join(__dirname, '../src/telemetry_dashboard/dashboard_component.rs'), 'utf8');
    assert.match(source, /gs26-tab-nav[^\n]+overflow-x:auto[^\n]+scrollbar-width:none/);
    assert.match(source, /gs26-tab-nav::-webkit-scrollbar[^\n]+display:none/);
});

test('mission tools stay in the application without popup or top-navigation permission',()=>{
    const source=fs.readFileSync(path.join(__dirname,'../src/telemetry_dashboard/live_stream_tab.rs'),'utf8');
    assert.doesNotMatch(source,/target:\s*"_blank"/);
    assert.match(source,/embedded_tool\.set\(Some\("\/radio"\)\)/);
    assert.match(source,/"sandbox":"allow-scripts allow-same-origin allow-forms allow-downloads"/);
});
