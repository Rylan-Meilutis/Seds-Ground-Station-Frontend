// Cancel a button activation when its press became a drag or scroll. Installed
// on Window in capture phase, before framework click delegation, in every shell.
// Tracking preserves scrolling and focus defaults. Only a cancelled legacy
// touchend has its activation default prevented. Keyboard/assistive clicks have
// no pointer gesture.
(() => {
  if (window.__gs26ActivationGestureGuard) return;
  window.__gs26ActivationGestureGuard = true;
  const activationSelector = 'button,[role="button"],input[type="button"],input[type="submit"],input[type="reset"],a[href],summary';
  const movementSquared = 8 * 8; // CSS pixels, independent of device scale.
  let gesture = null;
  const activation = event => {
    for (const node of event.composedPath()) {
      if (node instanceof Element && node.matches(activationSelector)) return node;
    }
    return null;
  };
  const offsets = button => {
    const nodes = [];
    for (let node = button; node; node = node.parentElement) {
      nodes.push([node, node.scrollLeft, node.scrollTop]);
    }
    nodes.push([window, window.scrollX, window.scrollY]);
    return nodes;
  };
  const scrolled = current => current.offsets.some(([node, x, y]) =>
    node === window ? window.scrollX !== x || window.scrollY !== y : node.scrollLeft !== x || node.scrollTop !== y);
  const begin = (event, id, x, y) => {
    // A new physical gesture retires a cancelled one, including a browser which
    // omitted the cancelled gesture's compatibility click.
    const button = activation(event);
    gesture = button ? { button, id, x, y, cancelled: false, offsets: offsets(button) } : null;
  };
  const move = (id, x, y) => {
    if (!gesture || gesture.id !== id) return;
    if ((x - gesture.x) ** 2 + (y - gesture.y) ** 2 > movementSquared || scrolled(gesture)) gesture.cancelled = true;
  };
  const cancel = () => { if (gesture) gesture.cancelled = true; };
  const listen = (type, handler) => window.addEventListener(type, handler, { capture: true, passive: true });
  if (window.PointerEvent) {
    listen('pointerdown', event => {
      if (!event.isPrimary) { cancel(); return; }
      if (event.button === 0) begin(event, event.pointerId, event.clientX, event.clientY);
    });
    listen('pointermove', event => move(event.pointerId, event.clientX, event.clientY));
    listen('pointerup', event => move(event.pointerId, event.clientX, event.clientY));
    listen('pointercancel', event => { if (gesture?.id === event.pointerId) cancel(); });
  } else {
    // Legacy WebViews use Touch Events. Cancel activation at touchend itself
    // so compatibility mouse events cannot rearm a cancelled touch gesture.
    listen('touchstart', event => {
      if (event.touches.length !== 1) { cancel(); return; }
      const touch = event.changedTouches[0];
      if (touch) begin(event, touch.identifier, touch.clientX, touch.clientY);
    });
    const touchMove = event => {
      for (const touch of event.changedTouches) move(touch.identifier, touch.clientX, touch.clientY);
    };
    listen('touchmove', touchMove);
    // Cancel only the end's activation default, never touchmove or ongoing
    // native scrolling. Touch Events requires a cancelled touchend not to
    // generate click/compatibility mouse events. This removes the ambiguous
    // old-WebView ghost-mouse stream without withholding a fresh mouse press.
    window.addEventListener('touchend', event => {
      touchMove(event);
      if (gesture && typeof gesture.id === 'number' && gesture.cancelled) {
        if (event.cancelable) event.preventDefault();
        gesture = null;
      }
    }, { capture: true, passive: false });
    // touchcancel has no activation default; fresh physical input starts anew.
    listen('touchcancel', () => { gesture = null; });
    listen('mousedown', event => {
      if (event.button === 0 && !event.sourceCapabilities?.firesTouchEvents)
        begin(event, 'mouse', event.clientX, event.clientY);
    });
    listen('mousemove', event => move('mouse', event.clientX, event.clientY));
    listen('mouseup', event => move('mouse', event.clientX, event.clientY));
  }
  listen('scroll', () => { if (gesture && scrolled(gesture)) cancel(); });
  listen('blur', event => { if (event.target === window) cancel(); });
  listen('dragstart', cancel);
  // Keyboard activation has its own lifecycle, and must never inherit an old
  // drag cancellation (Space activates on keyup; Enter usually on keydown).
  listen('keydown', event => { if (event.key === 'Enter' || event.key === ' ') gesture = null; });
  window.addEventListener('click', event => {
    const current = gesture;
    // detail zero is a keyboard/assistive/programmatic click, not a pointer tap.
    if (!current || (event.detail === 0 && !event.pointerType && !event.sourceCapabilities?.firesTouchEvents)) return;
    const button = activation(event);
    if (button !== current.button) return;
    gesture = null;
    if (current.cancelled || scrolled(current)) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, { capture: true });
})();
