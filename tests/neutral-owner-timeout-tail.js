const neutralTimers=new Set();
try{
  const context=await browser.newContext({viewport:{width:1440,height:1000},serviceWorkers:'block'});
  await context.addInitScript(({session,origin})=>{
    localStorage.setItem('navigation-style','tabs');
    localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
    window.__neutralTimeoutRequests=[];
    const originalTimeout=window.setTimeout;
    window.setTimeout=function(...args){
      if(args[1]===15000||args[1]===300000)window.__neutralTimeoutRequests.push({delay:args[1],at:performance.now()});
      return Reflect.apply(originalTimeout,this,args);
    };
  },{session,origin});
  await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
  await context.routeWebSocket(url=>url.pathname==='/ws'&&url.searchParams.get('token')==='ui-token',socket=>{
    const send=()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:{key_enabled:false,software_buttons_enabled:false,controls:[]}}));
    send();const timer=setInterval(send,500);neutralTimers.add(timer);
    socket.onClose(()=>{clearInterval(timer);neutralTimers.delete(timer);});
  });
  const page=await context.newPage(),errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  await page.goto(origin);
  await page.getByRole('button',{name:/Choose tab/}).waitFor();
  const settings=page.getByRole('link',{name:'Settings',exact:true}).or(page.getByRole('button',{name:/^Settings$/i}));
  if(!await settings.isVisible())await page.locator('.gs26-header-menu-toggle').filter({hasText:'Tools'}).click();
  await settings.click();
  await page.getByRole('button',{name:'Ground Station',exact:true}).click();
  await page.getByRole('navigation',{name:'Ground Station settings sections'}).getByRole('button',{name:'Administration',exact:true}).click();
  await page.evaluate(()=>{window.__neutralTimeoutRequests=[];});
  const started=Date.now();
  await page.getByRole('button',{name:'Manage users & access',exact:true}).click();
  await page.getByRole('heading',{name:'Administration',exact:true}).waitFor();
  await page.getByText('HTTP GET /api/admin/users timed out',{exact:false}).waitFor({timeout:22000});
  const elapsed=Date.now()-started;
  assert(elapsed>=14500&&elapsed<22000,'actual ordinary GET honors unchanged15s deadline');
  assert.equal(actualAdminReads,1,'no automatic neutral admin request retry');
  const scheduled=await page.evaluate(()=>window.__neutralTimeoutRequests);
  assert(scheduled.some(row=>row.delay===15000),'actual Wasm scheduled original normal GET timer');
  assert.equal(await page.getByText(/This panel has retired/).count(),0,'stable owner reports timeout, not false retirement');
  assert.deepEqual(errors,[]);
  // 300000ms remains source-bound contract only: current /api/recent/report
  // consumers use other saved-json entries; no invented timer/caller claim.
  console.log(JSON.stringify({scope:'actual-current-neutral-admin-wasm-timeout',actualAdminReads,elapsed,scheduled,errors,long300s:'source-contract-only-not-executed'}));
  for(const timer of neutralTimers)clearInterval(timer);
  await context.close();
}finally{
  for(const timer of neutralTimers)clearInterval(timer);
  for(const response of heldAdminReplies)response.destroy();
  await browser.close();
  await new Promise(resolve=>server.close(resolve));
}
