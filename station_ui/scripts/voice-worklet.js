// Built-in noise suppression for the 16 kHz capture stream, used where the
// platform has none (WebKit: Safari, the iOS/macOS apps and WebKitGTK do not
// offer the noiseSuppression constraint). Short-time Fourier transform,
// 256-point frames with a 128-sample hop and square-root Hann windows; the
// noise spectrum follows the running minimum of the smoothed power, and each
// bin gets a decision-directed Wiener gain floored at -20 dB. Delay: 256
// samples (16 ms). Steady noise (fans, wind, engines) drops by about 13 dB;
// a voice or tone well above the noise passes within 0.5 dB.
class NoiseSuppressor16k {
  // `floor` is the least gain per bin (0.1 = -20 dB); 1 passes audio unchanged.
  constructor(floor=0.1){
    this.floor=floor;this.n=256;this.hop=128;const n=this.n;
    this.window=new Float32Array(n);for(let i=0;i<n;i++)this.window[i]=Math.sqrt(0.5-0.5*Math.cos(2*Math.PI*i/n));
    this.re=new Float64Array(n);this.im=new Float64Array(n);
    this.bins=n/2+1;this.power=new Float64Array(this.bins);this.noise=new Float64Array(this.bins);
    this.prior=new Float64Array(this.bins);this.gain=new Float64Array(this.bins).fill(1);
    this.history=new Float32Array(n);this.pending=new Float32Array(this.hop);this.overlap=new Float32Array(this.hop);
    this.ready=new Float32Array(this.hop);this.pos=0;this.frames=0;
  }
  fft(re,im,inverse){
    const n=re.length;
    for(let i=1,j=0;i<n;i++){let bit=n>>1;for(;j&bit;bit>>=1)j^=bit;j^=bit;if(i<j){let t=re[i];re[i]=re[j];re[j]=t;t=im[i];im[i]=im[j];im[j]=t;}}
    for(let len=2;len<=n;len<<=1){
      const angle=(inverse?2:-2)*Math.PI/len,wr=Math.cos(angle),wi=Math.sin(angle);
      for(let i=0;i<n;i+=len){let cr=1,ci=0;for(let k=0;k<len/2;k++){const a=i+k,b=a+len/2,tr=re[b]*cr-im[b]*ci,ti=re[b]*ci+im[b]*cr;re[b]=re[a]-tr;im[b]=im[a]-ti;re[a]+=tr;im[a]+=ti;const nr=cr*wr-ci*wi;ci=cr*wi+ci*wr;cr=nr;}}
    }
    if(inverse)for(let i=0;i<n;i++){re[i]/=n;im[i]/=n;}
  }
  frame(){
    const {n,hop,re,im,window}=this;
    this.history.copyWithin(0,hop);this.history.set(this.pending,n-hop);
    for(let i=0;i<n;i++){re[i]=this.history[i]*window[i];im[i]=0;}
    this.fft(re,im,false);
    const first=this.frames++===0;
    for(let k=0;k<this.bins;k++){
      const p=re[k]*re[k]+im[k]*im[k];
      this.power[k]=first?p:0.7*this.power[k]+0.3*p;
      // Minimum tracking: fall at once, rise about 1 dB per second.
      this.noise[k]=first||this.power[k]<this.noise[k]?this.power[k]:this.noise[k]*1.0018+1e-12;
      // The running minimum sits below the mean noise power; twice it is close.
      const post=p/(2*this.noise[k]+1e-12);
      const prior=first?Math.max(post-1,0):0.98*this.gain[k]*this.gain[k]*this.prior[k]+0.02*Math.max(post-1,0);
      this.prior[k]=post;
      const g=Math.max(this.floor,prior/(1+prior));this.gain[k]=g;
      re[k]*=g;im[k]*=g;if(k>0&&k<n/2){re[n-k]*=g;im[n-k]*=g;}
    }
    this.fft(re,im,true);
    for(let i=0;i<hop;i++)this.ready[i]=this.overlap[i]+re[i]*window[i];
    for(let i=0;i<hop;i++)this.overlap[i]=re[i+hop]*window[i+hop];
  }
  process(sample){
    const out=this.ready[this.pos];this.pending[this.pos]=sample;
    if(++this.pos===this.hop){this.pos=0;this.frame();}
    return out;
  }
}
globalThis.NoiseSuppressor16k=NoiseSuppressor16k;

// Wire audio: 320 signed PCM16 samples at 16 kHz per packet (20 ms).
class CrewAudio extends AudioWorkletProcessor {
  constructor() {
    super(); this.transmit=false; this.gain=1; this.volume=1; this.peers=new Map();
    this.captureId=0; this.noise=null; this.samples=new Int16Array(320); this.used=0; this.phase=0; this.sum=0; this.n=0; this.meter=0;
    this.port.onmessage=({data:m})=>{
      if(m.type==='settings') { if(Number.isSafeInteger(m.capture_id)&&m.capture_id!==this.captureId){this.captureId=m.capture_id;this.used=0;this.phase=0;this.sum=0;this.n=0;} this.transmit=m.transmit; this.gain=m.gain; this.volume=m.volume; if(!this.transmit){this.used=0;this.phase=0;this.sum=0;this.n=0;} }
      // Built-in noise suppression on or off; a new instance starts clean.
      if(m.type==='noise') this.noise=m.enabled?new NoiseSuppressor16k():null;
      if(m.type==='remove') this.peers.delete(m.id);
      if(m.type==='reset') this.peers.clear();
      if(m.type==='audio') {
        let p=this.peers.get(m.id); if(!p){p={buffer:new Float32Array(4096),read:0,write:0,count:0,phase:0,playing:false,idle:0};this.peers.set(m.id,p);}p.idle=0;
        const view=new DataView(m.buffer);
        if(p.count+view.byteLength/2>3200){p.read=p.write;p.count=0;p.phase=0;p.playing=false;}
        for(let i=0;i<view.byteLength;i+=2){p.buffer[p.write]=view.getInt16(i,true)/32768;p.write=(p.write+1)%4096;p.count++;}
      }
    };
  }
  process(inputs, outputs) {
    const input=inputs[0]?.[0], output=outputs[0][0]; let peak=0;
    for(let i=0;i<output.length;i++) {
      const mic=Math.max(-1,Math.min(1,(input?.[i]||0)*this.gain));peak=Math.max(peak,Math.abs(mic));
      if(this.transmit) {
        this.sum+=mic;this.n++;this.phase+=16000;
        if(this.phase>=sampleRate) {
          this.phase-=sampleRate;let sample=this.sum/this.n;this.sum=0;this.n=0;if(this.noise)sample=Math.max(-1,Math.min(1,this.noise.process(sample)));
          this.samples[this.used++]=Math.round(sample*(sample<0?32768:32767));
          if(this.used===320){const buffer=this.samples.buffer;this.port.postMessage({type:'pcm',capture_id:this.captureId,buffer},[buffer]);this.samples=new Int16Array(320);this.used=0;}
        }
      }
      let mixed=0;
      for(const p of this.peers.values()) {
        if(!p.playing&&p.count>=640)p.playing=true; // 40 ms jitter prebuffer
        if(!p.playing)continue;
        if(p.count<2){p.count=0;p.read=p.write;p.phase=0;p.playing=false;continue;}
        mixed+=p.buffer[p.read]*(1-p.phase)+p.buffer[(p.read+1)%4096]*p.phase;
        p.phase+=16000/sampleRate;
        while(p.phase>=1&&p.count){p.phase--;p.read=(p.read+1)%4096;p.count--;}
      }
      output[i]=Math.max(-1,Math.min(1,mixed*this.volume));
    }
    for(const [id,p] of this.peers){if(!p.playing&&p.count<2){p.idle+=output.length;if(p.idle>sampleRate*5)this.peers.delete(id);}}
    this.meter+=output.length;
    if(this.meter>=sampleRate/10){this.port.postMessage({type:'level',value:peak});this.meter=0;}
    return true;
  }
}
registerProcessor('crew-audio',CrewAudio);
