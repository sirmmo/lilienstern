// Browser-to-browser play over WebRTC, as in OpenTactics. PeerJS (vendor/, MIT) only introduces
// the browsers through its public server; after that, messages go straight from one to the other.
//
// The host's browser is the referee: it puts every command (a decision, an order, the end of a
// day) in one sequence and sends it to every guest. Every browser runs the same deterministic
// engine on the same commands, and compares a fingerprint of the game with the host's after each
// day; a guest that differs, or comes back after a dropped connection, replays the sequence.

/* global Peer */
window.LilNet = (() => {
  const LETTERS = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  let script = null;

  function newCode() {
    return Array.from(crypto.getRandomValues(new Uint8Array(5)), (b) => LETTERS[b % LETTERS.length]).join('');
  }

  function cleanCode(text) {
    return (text || '').toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 8);
  }

  function loadPeerJs() {
    if (window.Peer) return Promise.resolve();
    return (script ||= new Promise((ok, fail) => {
      const s = document.createElement('script');
      s.src = 'vendor/peerjs.min.js';
      s.onload = ok;
      s.onerror = () => fail(new Error('Could not load PeerJS.'));
      document.head.append(s);
    }));
  }

  function openPeer(id) {
    return new Promise((ok, fail) => {
      const peer = id ? new Peer(id, { debug: 1 }) : new Peer({ debug: 1 });
      const timer = setTimeout(() => fail(new Error('The introduction server did not answer.')), 15000);
      peer.on('open', () => { clearTimeout(timer); ok(peer); });
      peer.on('error', (e) => {
        clearTimeout(timer);
        fail(new Error(e.type === 'unavailable-id' ? 'That room code is taken.' : e.message || String(e)));
      });
    });
  }

  /** Opens room `code` of game `prefix`; `onGuest(conn)` for every guest that connects. */
  async function host(prefix, code, onGuest) {
    await loadPeerJs();
    const peer = await openPeer(prefix + code);
    peer.on('connection', (conn) => conn.on('open', () => onGuest(conn)));
    peer.on('disconnected', () => { if (!peer.destroyed) setTimeout(() => peer.reconnect(), 1500); });
    return peer;
  }

  /** Joins room `code`: resolves with an open connection to the host. */
  async function join(prefix, code) {
    await loadPeerJs();
    const peer = await openPeer();
    // 'binary' splits big messages (a long game's commands) into chunks.
    const conn = peer.connect(prefix + code, { reliable: true, serialization: 'binary' });
    await new Promise((ok, fail) => {
      const timer = setTimeout(() => fail(new Error('No room with that code answered.')), 15000);
      conn.on('open', () => { clearTimeout(timer); ok(); });
      peer.on('error', (e) => {
        clearTimeout(timer);
        fail(new Error(e.type === 'peer-unavailable' ? 'No room with that code is open.' : e.message || String(e)));
      });
    });
    return { peer, conn };
  }

  /** Who this browser is, so a guest gets its family back after a dropped connection. */
  function token() {
    try {
      let t = sessionStorage.getItem('lilienstern.token');
      if (!t) {
        t = Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');
        sessionStorage.setItem('lilienstern.token', t);
      }
      return t;
    } catch {
      return Math.random().toString(36).slice(2);
    }
  }

  return { newCode, cleanCode, host, join, token };
})();
