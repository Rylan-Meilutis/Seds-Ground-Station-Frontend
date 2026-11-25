// Run with node tests/station-voice_audio.cjs. No microphone, network, or receiver required.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
let Processor;
const sandbox = {sampleRate:48000, AudioWorkletProcessor:class {constructor(){this.messages=[];this.port={postMessage:m=>this.messages.push(m)};}},registerProcessor:(_,c)=>{Processor=c;}};
vm.runInNewContext(fs.readFileSync('station_ui/scripts/voice-worklet.js','utf8'),sandbox);
const audio=new Processor();
const input=new Float32Array(128).fill(.25), output=new Float32Array(128);
const run=n=>{for(let i=0;i<n;i++)audio.process([[input]],[[output]]);};
run(375);
assert.equal(audio.messages.filter(m=>m.type==='pcm').length,0,'muted must never send audio');
audio.port.onmessage({data:{type:'settings',transmit:true,gain:2,volume:1}});
run(375); // exactly one second of capture at 48 kHz
const packets=audio.messages.filter(m=>m.type==='pcm');
assert.equal(packets.length,50);
assert.equal(packets[0].buffer.byteLength,640);
assert.equal(new DataView(packets[0].buffer).getInt16(0,true),16384);
audio.port.onmessage({data:{type:'settings',transmit:false,gain:1,volume:1}});
run(10);assert.equal(audio.messages.filter(m=>m.type==='pcm').length,50);
for(let i=0;i<2;i++)audio.port.onmessage({data:{type:'audio',id:1,buffer:packets[0].buffer}});
run(1);assert(output.some(x=>x>0),'received voice must reach playback');
audio.port.onmessage({data:{type:'settings',transmit:false,gain:1,volume:0}});
run(1);assert(output.every(x=>x===0),'speaker mute must silence playback');
for(let i=0;i<100;i++)audio.port.onmessage({data:{type:'audio',id:1,buffer:packets[0].buffer}});
assert(audio.peers.get(1).count<=3200,'jitter buffer must be bounded');
audio.port.onmessage({data:{type:'remove',id:1}});assert.equal(audio.peers.size,0);
console.log('PASS: resampling, PCM framing, PTT mute, gain, playback, speaker volume, and bounded jitter buffer');

// A microphone destination switch must discard a partial packet and tag all
// subsequent PCM so the host cannot send an old commentary frame into VC.
const switched = new Processor();
switched.port.onmessage({data:{type:'settings',transmit:true,gain:1,volume:0,capture_id:1}});
for(let i=0;i<3;i++)switched.process([[new Float32Array(128).fill(.1)]],[[new Float32Array(128)]]);
assert.equal(switched.messages.filter(m=>m.type==='pcm').length,0);
switched.port.onmessage({data:{type:'settings',transmit:true,gain:1,volume:0,capture_id:2}});
for(let i=0;i<8;i++)switched.process([[new Float32Array(128).fill(.5)]],[[new Float32Array(128)]]);
const changed=switched.messages.filter(m=>m.type==='pcm');assert.equal(changed.length,1);assert.equal(changed[0].capture_id,2);
assert.equal(new DataView(changed[0].buffer).getInt16(0,true),16384,'old destination partial samples cannot cross the boundary');
console.log('PASS: destination epochs reset partial microphone packets and tag completed PCM');

// Built-in noise suppression (WebKit has no noiseSuppression constraint).
// Off by default, so the commentary studio and other users are unchanged.
{
  const {NoiseSuppressor16k}=sandbox;
  assert.equal(typeof NoiseSuppressor16k,'function');
  let seed=1;const random=()=>{seed=(seed*1103515245+12345)%2147483648;return seed/2147483648*2-1;};
  const suppressor=new NoiseSuppressor16k();const second=16000;
  let noiseIn=0,noiseOut=0,toneIn=0,toneOut=0;
  for(let i=0;i<second*4;i++){
    const noise=0.05*random(),tone=i>=second*3?0.3*Math.sin(2*Math.PI*440*i/16000):0,out=suppressor.process(noise+tone);
    if(i>=second*2&&i<second*3){noiseIn+=noise*noise;noiseOut+=out*out;}
    if(i>=second*3+4000){toneIn+=(noise+tone)**2;toneOut+=out*out;}
  }
  const noiseDb=10*Math.log10(noiseOut/noiseIn),toneDb=10*Math.log10(toneOut/toneIn);
  assert(noiseDb<-10,`steady noise drops by more than 10 dB (${noiseDb.toFixed(1)} dB)`);
  assert(Math.abs(toneDb)<0.5,`a tone well above the noise passes (${toneDb.toFixed(2)} dB)`);
  // At unity gain the windows reconstruct the input exactly, 256 samples (16 ms) later.
  const passthrough=new NoiseSuppressor16k(1),input=Array.from({length:2048},()=>random()),delayed=input.map(x=>passthrough.process(x));
  for(let i=512;i<2048;i++)assert(Math.abs(delayed[i]-input[i-256])<1e-5,`sample ${i}`);
  const off=new Processor();off.port.onmessage({data:{type:'settings',transmit:true,gain:1,volume:0}});
  const on=new Processor();on.port.onmessage({data:{type:'settings',transmit:true,gain:1,volume:0}});on.port.onmessage({data:{type:'noise',enabled:true}});
  for(let i=0;i<375*2;i++){const block=new Float32Array(128).map(()=>0.05*random());off.process([[block]],[[new Float32Array(128)]]);on.process([[block]],[[new Float32Array(128)]]);}
  const energy=p=>p.messages.filter(m=>m.type==='pcm').slice(-25).reduce((sum,m)=>{const v=new Int16Array(m.buffer);return sum+v.reduce((a,x)=>a+x*x,0);},0);
  assert(energy(on)<energy(off)/5,'the toggle puts the suppressor in the capture path');
  on.port.onmessage({data:{type:'noise',enabled:false}});assert.equal(on.noise,null);
  console.log(`PASS: built-in noise suppression (${noiseDb.toFixed(1)} dB on steady noise, ${toneDb.toFixed(2)} dB on a tone, 16 ms delay) and its toggle`);
}
