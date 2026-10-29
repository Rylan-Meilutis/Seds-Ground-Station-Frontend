// Browser failover uses paired HTTP origins; raw LAN broadcast is native-only.
(() => {
  if (window.GS26StationDiscovery) return;
  let state = null, timer = null, busy = false, failures = 0;
  const decode = value => Uint8Array.from(atob(value.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
  function origin(value) {
    try {
      const u = new URL(value);
      return ['http:', 'https:'].includes(u.protocol) && !u.username && !u.password && u.pathname === '/' && !u.search && !u.hash ? u.origin : null;
    } catch (_) { return null; }
  }
  async function probe(base, nonce, trust) {
    const controller = new AbortController(), timeout = setTimeout(() => controller.abort(), 1200);
    try {
      const response = await fetch(`${base}/api/station/discovery?nonce=${nonce}`, {signal:controller.signal, credentials:'omit', cache:'no-store', redirect:'error'});
      if (!response.ok) return null;
      let text;
      if (response.body && response.body.getReader) {
        const reader = response.body.getReader(), chunks = []; let length = 0;
        try {
          while (true) {
            const {done,value} = await reader.read(); if (done) break;
            length += value.length;
            if (length > 8192) {await reader.cancel(); return null;}
            chunks.push(value);
          }
        } finally {reader.releaseLock();}
        const bytes = new Uint8Array(length); let offset = 0;
        for (const chunk of chunks) {bytes.set(chunk,offset);offset += chunk.length;}
        text = new TextDecoder().decode(bytes);
      } else {
        text = await response.text(); if (text.length > 8192) return null;
      }
      const e = JSON.parse(text);
      const key = await crypto.subtle.importKey('raw', decode(trust.verify_key), {name:'Ed25519'}, false, ['verify']);
      if (!await crypto.subtle.verify({name:'Ed25519'}, key, decode(e.signature), new TextEncoder().encode(`station1.${e.payload}`))) return null;
      const identity = JSON.parse(new TextDecoder().decode(decode(e.payload)));
      if (identity.nonce !== nonce || identity.installation_id !== trust.installation_id) return null;
      return identity;
    } catch (_) { return null; } finally { clearTimeout(timeout); }
  }
  async function check() {
    if (busy || !state || document.hidden || !crypto.subtle) return;
    busy = true;
    const trust = state;
    try {
      const bytes = crypto.getRandomValues(new Uint8Array(24));
      const nonce = Array.from(bytes, b => b.toString(16).padStart(2,'0')).join('');
      const local = await probe(location.origin, nonce, trust);
      if (state !== trust) return;
      if (local && local.station_id === trust.station_id) { failures = 0; return; }
      if (++failures < 3) return;
      const peers = trust.peers.slice(0,8).flatMap(peer => (peer.urls || []).slice(0,8).map(url => ({peer,base:origin(url)}))).filter(p => p.base && p.base !== location.origin).slice(0,8);
      const results = await Promise.all(peers.map(async p => ({...p,identity:await probe(p.base,nonce,trust)})));
      const match = results.find(p => p.identity && p.identity.station_id === p.peer.id && p.identity.role === p.peer.role);
      if (match && state === trust) location.replace(`${match.base}/`);
    } finally { busy = false; }
  }
  function configure(pairing) {
    if (!pairing || typeof pairing.installation_id !== 'string' || typeof pairing.station_id !== 'string' || typeof pairing.verify_key !== 'string' || !Array.isArray(pairing.peers)) return;
    if (!state || state.installation_id !== pairing.installation_id || state.station_id !== pairing.station_id || state.verify_key !== pairing.verify_key) failures = 0;
    state = pairing;
    try {localStorage.setItem('gs26-paired-station-discovery', JSON.stringify(pairing));} catch (_) {}
    if (timer === null) timer = setInterval(check, 2500);
  }
  function stop() {if (timer !== null) clearInterval(timer); timer = null; state = null; failures = 0;}
  window.GS26StationDiscovery = {configure,check,stop};
  try {configure(JSON.parse(localStorage.getItem('gs26-paired-station-discovery')));} catch (_) {}
  document.addEventListener('visibilitychange', () => {if (!document.hidden) check();});
})();
