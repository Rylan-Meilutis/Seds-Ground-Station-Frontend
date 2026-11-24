// Demo mode at /preview: the playback bar's T clock and the dashboard
// header's T clock are one clock (the local station's LaunchClock), at 1x,
// at 10x, after a seek and paused. The flight maxima cards fill in from the
// rows once the clock counts up.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const shots=process.env.GS_UI_ARTIFACTS;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
// "T- 00:04.75" / "(T+:" + "01:18.40" -> signed milliseconds from T-0.
const signed=(label,body)=>{const m=/^(\d+):(\d{2})\.(\d{2})$/.exec(body.trim());assert(m,`clock digits: ${body}`);
 const ms=(+m[1]*60+ +m[2])*1000+ +m[3]*10;return label.includes('+')?ms:-ms;};
try {
 const page=await browser.newPage({viewport:{width:1280,height:860}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(base+'/preview');
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 await page.locator('.gs26-launch-clock-value').first().waitFor();
 // Both clocks read in the same frame.
 const read=()=>page.evaluate(()=>{
  const bar=document.querySelector('[data-testid=local-clock]').textContent.trim();
  const clock=document.querySelector('.gs26-launch-clock');
  return {bar,label:clock.firstElementChild.textContent,value:clock.querySelector('.gs26-launch-clock-value').textContent};});
 const sample=async()=>{const r=await read();const [sign,body]=r.bar.split(' ');
  return {...r,barMs:signed(sign,body),headerMs:signed(r.label,r.value)};};
 const seek=async ms=>{await page.getByLabel('Position',{exact:true}).fill(String(ms));
  // The dashboard remounts at the new position; wait for the header to follow.
  await page.waitForFunction(()=>document.querySelector('.gs26-launch-clock-value'));await page.waitForTimeout(600);};
 const agree=async(what,speed,samples=8)=>{let worst=0;
  for(let i=0;i<samples;i++){const s=await sample();const diff=Math.abs(s.barMs-s.headerMs);worst=Math.max(worst,diff);
   // Each side updates on a 100 ms tick: at most a couple of ticks of playback apart.
   assert(diff<=speed*200+100,`${what}: bar ${s.bar} vs header ${s.label}${s.value} (${diff} ms apart)`);await page.waitForTimeout(170);}
  console.log(`${what}: worst ${worst} ms apart`);return worst;};
 // Idle before the countdown: the countdown length on both.
 let s=await sample();
 assert.equal(s.bar,'T- 00:10.00');assert.equal(`${s.label}${s.value}`,'(T-:00:10.00');
 // 1x through the countdown and over T-0.
 await seek(133000);
 s=await sample();assert(s.barMs<0&&s.headerMs<0,`counting down: ${JSON.stringify(s)}`);
 await agree('1x countdown',1);
 await page.waitForFunction(()=>document.querySelector('[data-testid=local-clock]').textContent.startsWith('T+'),null,{timeout:15000});
 await agree('1x after T-0',1);
 if(shots)await page.screenshot({path:`${shots}/tclock-agreement-1x.png`});
 // 10x: the header runs at playback speed, not wall speed.
 await page.getByLabel('Speed',{exact:true}).selectOption('10');
 const start=await sample();await page.waitForTimeout(1500);const end=await sample();
 assert(end.headerMs-start.headerMs>=10000,`the header runs at 10x: ${start.headerMs} -> ${end.headerMs}`);
 await agree('10x',10);
 if(shots)await page.screenshot({path:`${shots}/tclock-agreement-10x.png`});
 // A seek at 10x.
 await seek(150000);
 s=await sample();assert(s.barMs>=10000&&s.barMs<=25000,`after the seek: ${s.bar}`);
 await agree('10x after a seek',10);
 // Paused: both stop, on the same reading.
 await page.getByRole('button',{name:'Pause',exact:true}).click();await page.waitForTimeout(500);
 const paused=await sample();await page.waitForTimeout(1500);const still=await sample();
 assert.equal(paused.barMs,paused.headerMs,`paused: bar ${paused.bar} vs header ${paused.value}`);
 assert.deepEqual(still,paused,'a paused clock holds');
 if(shots)await page.screenshot({path:`${shots}/tclock-agreement-paused.png`});
 // A seek while paused: the new position on both, exactly.
 await seek(136000);
 s=await sample();assert.equal(s.bar,'T- 00:04.00');assert.equal(s.headerMs,s.barMs,`paused seek: ${JSON.stringify(s)}`);
 // The flight maxima cards fill in from the rows once the clock counts up
 // (a reconnect at a paused position carries them too).
 await seek(200000);
 const nav=page.locator('nav[aria-label="Mission navigation"]');
 await nav.locator('a[aria-label="Dashboard"]').click();
 const card=page.locator('.gs26-maxima-card[data-quantity="0"] .gs26-maxima-value');
 await card.first().waitFor({timeout:20000});
 assert.match(await card.first().innerText(),/^\d+ m · T\+\d+\.\d s · /,'max altitude with its T+ time and source');
 for(const q of ['1','2'])await page.locator(`.gs26-maxima-card[data-quantity="${q}"] .gs26-maxima-value`).first().waitFor({timeout:20000});
 if(shots)await page.screenshot({path:`${shots}/flight-maxima-demo.png`});
 assert.deepEqual(errors,[]);
 console.log('tclock agreement browser test PASS');
} finally { await browser.close(); }
