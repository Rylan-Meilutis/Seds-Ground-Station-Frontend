const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {webcrypto}=require('node:crypto');
(async()=>{
 const key=await webcrypto.subtle.generateKey({name:'Ed25519'},true,['sign','verify']);
 const pub=Buffer.from(await webcrypto.subtle.exportKey('raw',key.publicKey)).toString('base64url');
 for(const scenario of ['healthy','failover','forged','wrong_role','wrong_nonce','large_body']){
  let redirected=null,requests=[];
  const trust={installation_id:'test',station_id:'primary',verify_key:pub,peers:[{id:'rrls',role:'rrls',urls:['http://rrls.local:3000']}]};
  const context={window:{},document:{hidden:false,addEventListener(){}},location:{origin:'https://gs.local',replace:url=>{redirected=url;}},crypto:webcrypto,AbortController,TextEncoder,TextDecoder,URL,Uint8Array,atob,btoa,setTimeout,clearTimeout,setInterval:()=>1,clearInterval(){},localStorage:{getItem:()=>null,setItem(){}}};
  context.fetch=async(url,options)=>{
   requests.push({url,options});assert.equal(options.credentials,'omit');assert.equal(options.redirect,'error');assert(!options.headers,'discovery must not send a login token');
   const u=new URL(url),primary=u.hostname==='gs.local';
   if(primary&&scenario!=='healthy')throw new Error('offline');
   if(!primary&&scenario==='large_body')return new Response('x'.repeat(9000));
   const data={installation_id:'test',station_id:primary?'primary':'rrls',role:primary?'primary':'rrls',nonce:u.searchParams.get('nonce'),scheme:'http',port:3000};
   if(!primary&&scenario==='wrong_role')data.role='primary';
   if(!primary&&scenario==='wrong_nonce')data.nonce='old-reply';
   const payload=Buffer.from(JSON.stringify(data)).toString('base64url');
   const signature=Buffer.from(await webcrypto.subtle.sign({name:'Ed25519'},key.privateKey,new TextEncoder().encode(`${scenario==='forged'?'wrong':'station1'}.${payload}`))).toString('base64url');
   return new Response(JSON.stringify({payload,signature}));
  };
  vm.createContext(context);vm.runInContext(fs.readFileSync('static/station_discovery.js','utf8'),context);
  context.window.GS26StationDiscovery.configure(trust);
  for(let i=0;i<3;i++)await context.window.GS26StationDiscovery.check();
  assert.equal(redirected,scenario==='failover'?'http://rrls.local:3000/':null,scenario);
  if(scenario==='healthy')assert(requests.every(r=>new URL(r.url).hostname==='gs.local'));
  context.window.GS26StationDiscovery.stop();
 }
 console.log('PASS: healthy station retained, paired HTTP redirect after sustained loss, forged/stale/wrong-role discovery rejected, no session tokens in discovery');
})().catch(e=>{console.error(e);process.exitCode=1});
