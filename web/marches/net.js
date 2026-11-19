// Online play through Lilienstern's game server (crates/lilienstern-server): one WebSocket to
// it, which comes back by itself when the line drops.
//
// The server does not run the game: every browser does, with the same engine on the same
// files. The server puts every player's commands (a decision, an order, the end of a day) in
// one order, keeps them, and sends them to everyone; the log of commands is the game. A browser
// that falls behind, or comes back after a dropped connection, gets the commands it misses.

window.LilNet = (() => {
  const SERVER = 'wss://lilienstern.fantasymaps.org/ws';

  function cleanCode(text) {
    return (text || '').toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 8);
  }

  /** The server to use: `?server=` on the page's address (for tests), or the game's own. */
  function serverUrl() {
    return new URLSearchParams(location.search).get('server') || window.GAME?.server || SERVER;
  }

  /**
   * Opens a connection: resolves once it is open. `onMessage(msg)` gets every message;
   * `onState('lost' | 'back')` hears when the line drops and when it is back (then the page
   * says hello again: the server does not remember connections, only players).
   */
  function connect(onMessage, onState) {
    const url = serverUrl();
    let ws = null;
    let tries = 0;
    let closed = false;
    const conn = {
      /** Sends a message; false if the line is down (it will be back). */
      send(msg) {
        if (!ws || ws.readyState !== 1) return false;
        ws.send(JSON.stringify(msg));
        return true;
      },
      get open() { return !!ws && ws.readyState === 1; },
      close() { closed = true; if (ws) ws.close(); },
    };
    return new Promise((ok, fail) => {
      let first = true;
      const open = () => {
        ws = new WebSocket(url);
        const timer = setTimeout(() => { if (ws.readyState === 0) ws.close(); }, 10000);
        ws.onopen = () => {
          clearTimeout(timer);
          tries = 0;
          if (first) { first = false; ok(conn); } else onState('back');
        };
        ws.onmessage = (e) => {
          let msg;
          try { msg = JSON.parse(e.data); } catch { return; }
          onMessage(msg);
        };
        ws.onclose = () => {
          clearTimeout(timer);
          if (closed) return;
          if (first) { first = false; fail(new Error('The game server did not answer: try again in a moment.')); return; }
          if (tries === 0) onState('lost');
          setTimeout(open, Math.min(10000, 500 * 2 ** tries++));
        };
      };
      open();
    });
  }

  /** Who this browser is, so a player gets their family back after a dropped connection. */
  let mine = null;
  function token() {
    if (mine) return mine;
    try {
      let t = sessionStorage.getItem('lilienstern.token');
      if (!t) {
        t = Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');
        sessionStorage.setItem('lilienstern.token', t);
      }
      mine = t;
    } catch {
      mine = Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');
    }
    return mine;
  }

  return { cleanCode, connect, token, serverUrl };
})();
