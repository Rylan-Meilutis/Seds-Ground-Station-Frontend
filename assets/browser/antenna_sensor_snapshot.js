(() => {
const s=window.__gs26aim;const now=performance.now();dioxus.send(s?{error:s.error,orientation:s.orientation?{...s.orientation,age_ms:now-s.orientation.at}:null,position:s.position?{...s.position,age_ms:Math.floor(s.position.age_ms+now-s.position.at)}:null}:{});
})();
