const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const zlib=require('node:zlib');
const source=fs.readFileSync(path.join(__dirname,'../static/ground_map.js'),'utf8');
function runtime(fetch){
 const context=vm.createContext({window:{addEventListener(){},localStorage:{getItem(){return null;}}},document:{},navigator:{},console,Uint8Array,ArrayBuffer,URL,Headers,Response,fetch,setTimeout,clearTimeout,setInterval,clearInterval,requestAnimationFrame:fn=>fn(),performance});
 vm.runInContext(source,context);return context;
}
function crc32(bytes){let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let i=0;i<8;i++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return (crc^0xffffffff)>>>0;}
test('missing-tile fallback is a valid fully transparent PNG',()=>{
 const c=runtime();const png=Buffer.from(vm.runInContext('transparentTileFallbackArrayBuffer()',c));
 assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
 for(let offset=8;offset<png.length;){
  const length=png.readUInt32BE(offset);const name=png.subarray(offset+4,offset+8).toString();const data=png.subarray(offset+8,offset+8+length);
  assert.equal(crc32(png.subarray(offset+4,offset+8+length)),png.readUInt32BE(offset+8+length),name+' CRC');
  if(name==='IDAT')assert.deepEqual(zlib.inflateSync(data),Buffer.from([0,0,0,0,0]));
  offset+=length+12;
 }
});
test('empty, HTML and SVG responses never reach the raster decoder',async()=>{
 for(const response of [new Response(null,{status:204}),new Response(''),new Response('<html>login</html>'),new Response('<svg xmlns="http://www.w3.org/2000/svg"></svg>')]){
  const c=runtime(async()=>response);
  await assert.rejects(vm.runInContext("fetchRasterTile('cache','https://example.test/tile.jpg')",c),/tile fetch failed/);
 }
 const c=runtime();c.fetch=async()=>new Response(vm.runInContext('transparentTileFallbackArrayBuffer()',c),{headers:{'Content-Type':'image/png'}});
 const response=await vm.runInContext("fetchRasterTile('cache','https://example.test/tile.jpg')",c);
 assert.equal(response.status,200);
});
test('invalid legacy cached tiles are evicted individually',async()=>{
 const c=runtime();const deleted=[];c.cache={delete:async key=>deleted.push(key)};
 vm.runInContext('runBrowserTileCacheOp=async fn=>await fn();openTileCache=async()=>cache;',c);
 c.cached=new Response('<html>old fallback</html>');
 assert.equal(await vm.runInContext("readValidatedCachedTile('cache','https://example.test/bad.jpg',cached)",c),null);
 assert.deepEqual(deleted,['https://example.test/bad.jpg']);
 c.cached=new Response(vm.runInContext('transparentTileFallbackArrayBuffer()',c));
 assert((await vm.runInContext("readValidatedCachedTile('cache','https://example.test/good.jpg',cached)",c)).byteLength>0);
 assert.equal(deleted.length,1);
});

test('browser HTTP tile sources use the validating protocol',()=>{
 const c=runtime();
 c.window.location={protocol:'https:'};
 for(const url of ['https://station.test/tiles/{z}/{x}/{y}.jpg','http://station.test/tiles/{z}/{x}/{y}.jpg']) {
  c.url=url;
  assert.equal(vm.runInContext('shouldUseNativeTileTemplate(url)',c),false);
 }
});
