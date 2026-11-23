// Test-side observation only. The page executes Gazette's actual browser modules.
import { readFile } from 'node:fs/promises';
const [url, profile] = process.argv.slice(1);
const deadline = Date.now() + 30_000;
const maximum = 2 * 1024 * 1024;
const pause = () => new Promise(resolve => setTimeout(resolve, 20));
let socket;
let last = 'waiting for exact owned browser page';
const timeout = setTimeout(() => { console.error(`Native WebMCP deadline: ${last}`); process.exit(1); }, 30_000);
const remaining = () => Math.max(1, deadline - Date.now());
try {
  let port;
  while (Date.now() < deadline) {
    try {
      const text = await readFile(`${profile}/DevToolsActivePort`, 'utf8');
      if (text.length > 1024) throw Error('unbounded browser endpoint file');
      const candidate = text.split('\n')[0];
      if (!/^\d+$/.test(candidate) || +candidate < 1 || +candidate > 65535) throw Error('invalid owned browser endpoint');
      port = candidate; break;
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await pause();
  }
  if (!port) throw Error('owned browser endpoint unavailable');
  let page;
  while (Date.now() < deadline) {
    const response = await fetch(`http://127.0.0.1:${port}/json/list`, {signal:AbortSignal.timeout(remaining())});
    if (!response.ok) throw Error('owned debugging endpoint refused page discovery');
    if (!response.body) throw Error('page discovery body missing');
    const reader = response.body.getReader();
    const chunks = [];
    let bytes = 0;
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > maximum) { await reader.cancel(); throw Error('unbounded page discovery'); }
      chunks.push(chunk.value);
    }
    const body = new TextDecoder('utf-8', {fatal:true}).decode(Buffer.concat(chunks));
    page = JSON.parse(body).find(candidate => candidate.type === 'page' && candidate.url === url);
    if (page) break;
    await pause();
  }
  if (!page || !page.webSocketDebuggerUrl.startsWith(`ws://127.0.0.1:${port}/devtools/page/`)) throw Error('exact loopback page missing');
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve,reject) => {
    socket.addEventListener('open',resolve,{once:true});
    socket.addEventListener('error',reject,{once:true});
  });
  let id = 0;
  let completed = false;
  while (Date.now() < deadline) {
    const request = ++id;
    const response = new Promise((resolve,reject) => {
      const receive = event => {
        try {
          if (typeof event.data !== 'string' || Buffer.byteLength(event.data) > 2 * maximum) throw Error('unbounded native protocol message');
          const message = JSON.parse(event.data);
          if (message.id === request) { socket.removeEventListener('message',receive); resolve(message); }
        } catch (error) { socket.removeEventListener('message',receive); reject(error); }
      };
      socket.addEventListener('message',receive);
    });
    socket.send(JSON.stringify({id:request,method:'Runtime.evaluate',params:{
      // Only synchronous observation: awaited document-side evaluation can be destroyed
      // by native tool execution, and dump-DOM does not wait registration promises.
      expression:`(() => {const text=document.getElementById('webmcp-observation')?.textContent||document.readyState;return {ready:/^WEBMCP_(OBSERVATION|FAILURE|UNSUPPORTED):/.test(text),stage:text.slice(0,512),html:/^WEBMCP_(OBSERVATION|FAILURE|UNSUPPORTED):/.test(text)?'<!DOCTYPE html>'+document.documentElement.outerHTML:null};})()`,
      returnByValue:true,
    }}));
    const observed = await response;
    if (observed.error) throw Error(`native observation refused: ${JSON.stringify(observed.error)}`);
    if (observed.result.exceptionDetails) throw Error('native observation raised an exception');
    const value = observed.result.result.value;
    last = value.stage;
    if (value.ready) {
      if (typeof value.html !== 'string' || Buffer.byteLength(value.html) > maximum) throw Error('unbounded native DOM');
      process.stdout.write(value.html);
      completed = true;
      break;
    }
    await pause();
  }
  if (!completed) throw Error(`Native WebMCP deadline: ${last}`);
} catch (error) { console.error(error.message); process.exitCode = 1; }
finally { clearTimeout(timeout); socket?.close(); }
