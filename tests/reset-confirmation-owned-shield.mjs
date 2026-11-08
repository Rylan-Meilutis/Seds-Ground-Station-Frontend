import assert from 'node:assert/strict';
import fs from 'node:fs';

export async function verifyOwnedResetShield({commandPage,commandContext,actionButton,requests}) {
  // The original exact-count reset inventory above is unchanged. A retired
  // dialog owns only its former rectangle, never any command authority.
  const ownedShieldResults=[];
  async function openOwnedReset(cmd='ResetFlightState') {
    await commandPage.locator('[data-confirmation-retired="true"]').waitFor({state:'detached'});
    await actionButton(cmd).click();
    const dialog=commandPage.getByRole('alertdialog',{name:'Confirm reset'});
    await dialog.waitFor({state:'visible'});
    assert.equal(await dialog.getAttribute('data-confirmation-retired'),'false');
    return dialog;
  }
  for(const method of ['cancel-double','escape','keyboard-repeat']) {
    const count=requests.length;const dialog=await openOwnedReset();
    const confirm=dialog.getByRole('button',{name:'Confirm reset',exact:true});
    const old=await confirm.boundingBox();
    if(method==='cancel-double')await dialog.getByRole('button',{name:'Cancel',exact:true}).click({clickCount:2});
    if(method==='escape')await commandPage.keyboard.press('Escape');
    if(method==='keyboard-repeat'){await confirm.focus();await commandPage.keyboard.down('Enter');await commandPage.keyboard.down('Enter');await commandPage.keyboard.up('Enter');}
    const shield=commandPage.locator('[data-confirmation-retired="true"]');await shield.waitFor({state:'visible'});
    assert.equal(await shield.evaluate(e=>getComputedStyle(e.parentElement).pointerEvents),'none');
    assert.equal(await shield.evaluate(e=>getComputedStyle(e).pointerEvents),'auto');
    await commandPage.mouse.click(old.x+old.width/2,old.y+old.height/2);
    await shield.waitFor({state:'detached'});
    const expected=method==='keyboard-repeat'?['ResetFlightState']:[];
    assert.deepEqual(requests.slice(count),expected,method+' cannot click through after retirement');
    ownedShieldResults.push({method,requests:requests.slice(count)});
  }
  // The transparent parent must not steal a deliberate emergency Stop outside
  // the old dialog. Actual pointer input, not a programmatic command fallback.
  {
    const count=requests.length;const dialog=await openOwnedReset();
    await dialog.getByRole('button',{name:'Cancel',exact:true}).click();
    const shield=commandPage.locator('[data-confirmation-retired="true"]');await shield.waitFor({state:'visible'});
    const abort=actionButton('Abort');await abort.evaluate(e=>e.scrollIntoView({block:'start'}));
    const a=await abort.boundingBox(),r=await shield.boundingBox();
    const center={x:a.x+a.width/2,y:a.y+a.height/2};
    assert(!(center.x>=r.x&&center.x<=r.x+r.width&&center.y>=r.y&&center.y<=r.y+r.height),'actual Abort target outside former dialog');
    await abort.click();
    for(let i=0;i<100&&requests.length===count;i++)await commandPage.waitForTimeout(10);
    assert.deepEqual(requests.slice(count),['Abort'],'outside emergency Abort remains available');
    await shield.waitFor({state:'detached'});
    ownedShieldResults.push({method:'outside-emergency-Abort',requests:requests.slice(count)});
  }
  {
    const count=requests.length;let dialog=await openOwnedReset();
    await dialog.getByRole('button',{name:'Cancel',exact:true}).click();
    await commandPage.locator('[data-confirmation-retired="true"]').waitFor({state:'visible'});
    await actionButton('ResetTClock').click();
    dialog=commandPage.getByRole('alertdialog',{name:'Confirm reset'});
    assert.equal(await dialog.getAttribute('data-confirmation-retired'),'false');
    await commandPage.waitForTimeout(500);
    assert.equal(await dialog.getAttribute('data-confirmation-retired'),'false','old shield cleanup cannot consume a new confirmation');
    await dialog.getByRole('button',{name:'Confirm reset',exact:true}).click({clickCount:2});
    await commandPage.locator('[data-confirmation-retired="true"]').waitFor({state:'detached'});
    assert.deepEqual(requests.slice(count),['ResetTClock'],'new explicit dialog owns only its own single reset');
    ownedShieldResults.push({method:'new-dialog-old-cleanup-ABA',requests:requests.slice(count)});
  }
  {
    const count=requests.length;const dialog=await openOwnedReset();
    await commandPage.evaluate(()=>localStorage.setItem('gs_base_url',location.origin.replace('127.0.0.1','localhost')));
    await dialog.getByRole('button',{name:'Confirm reset',exact:true}).click({clickCount:2});
    await commandPage.locator('[data-confirmation-retired="true"]').waitFor({state:'detached'});
    assert.deepEqual(requests.slice(count),[],'station mismatch retires without dispatch or click-through');
    await commandPage.evaluate(()=>localStorage.setItem('gs_base_url',location.origin));
    ownedShieldResults.push({method:'stale-station-confirm',requests:requests.slice(count)});
  }
  {
    const count=requests.length;const dialog=await openOwnedReset();
    const confirm=dialog.getByRole('button',{name:'Confirm reset',exact:true});const b=await confirm.boundingBox();
    await commandPage.evaluate(()=>{window.resetTouchEvents=[];for(const name of ['touchstart','mousedown','click'])document.addEventListener(name,e=>window.resetTouchEvents.push({type:name,trusted:e.isTrusted}),true);});
    const cdp=await commandContext.newCDPSession(commandPage);
    await cdp.send('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:1});
    const points=[{x:b.x+b.width/2,y:b.y+b.height/2}];
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:points});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    const shield=commandPage.locator('[data-confirmation-retired="true"]');await shield.waitFor({state:'visible'});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:points});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await shield.waitFor({state:'detached'});
    const events=await commandPage.evaluate(()=>window.resetTouchEvents);
    assert(events.some(e=>e.type==='touchstart'&&e.trusted),'actual browser touch input hit');
    assert(events.some(e=>e.type==='mousedown'&&e.trusted),'actual compatibility mouse input hit');
    assert.deepEqual(requests.slice(count),['ResetFlightState'],'touch+compatibility and second tap cannot click through');
    await cdp.send('Emulation.setTouchEmulationEnabled',{enabled:false});await cdp.detach();
    ownedShieldResults.push({method:'real-touch-double-tap-compatibility',requests:requests.slice(count),events});
  }
  if(process.env.GS_RESET_SHIELD_RESULT)fs.writeFileSync(process.env.GS_RESET_SHIELD_RESULT,JSON.stringify(ownedShieldResults,null,2));
}

export async function verifyRetiredResetAuth({commandPage,actionButton,requests}) {
  // The full fixture has legitimately navigated away from Actions by here.
  // Return through the actual current navigation before staging a pending reset.
  if (await actionButton('ResetFlightState').count() === 0) {
    const rail=commandPage.locator('.gs26-navigation-rail');
    if (await rail.count()) await rail.getByRole('link',{name:'Actions',exact:true}).click();
    else {
      await commandPage.getByRole('button',{name:/Choose tab/}).click();
      await commandPage.getByRole('button',{name:'Actions',exact:true}).click();
    }
  }
  {
    await commandPage.locator('[data-confirmation-retired="true"]').waitFor({state:'detached'});
    const count=requests.length;await actionButton('ResetFlightState').click();
    const dialog=commandPage.getByRole('alertdialog',{name:'Confirm reset'});await dialog.waitFor({state:'visible'});
    const stale=await dialog.getByRole('button',{name:'Confirm reset',exact:true}).elementHandle();
    await commandPage.getByRole('button',{name:/^Sign out/,includeHidden:true}).evaluate(e=>e.click());
    await actionButton('ResetFlightState').waitFor({state:'detached'});
    await stale.evaluate(e=>e.click());
    await commandPage.waitForTimeout(500);
    assert.deepEqual(requests.slice(count),[],'actual signout/unmount retires pending reset with no late authority');
    const ownedShieldResults=process.env.GS_RESET_SHIELD_RESULT?JSON.parse(fs.readFileSync(process.env.GS_RESET_SHIELD_RESULT,'utf8')):[];
    ownedShieldResults.push({method:'auth-signout-unmount-stale-confirm',requests:requests.slice(count)});
    if(process.env.GS_RESET_SHIELD_RESULT)fs.writeFileSync(process.env.GS_RESET_SHIELD_RESULT,JSON.stringify(ownedShieldResults,null,2));
  }
}
