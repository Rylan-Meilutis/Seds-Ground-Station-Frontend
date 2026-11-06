// Actual current neutral admin GET deadline; no shortened timers or fake clock.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const sourcePath='tests/dashboard-customization.mjs';
const original=fs.readFileSync(sourcePath,'utf8');
const boundary=original.indexOf('try{\n  const context=');
assert(boundary>0,'exact shared fixture boundary');
let fixture=original.slice(0,boundary);
const replace=(before,after)=>{
  assert.equal(fixture.split(before).length-1,1,'unique original fixture anchor: '+before);
  fixture=fixture.replace(before,after);
};
replace("from './reset-confirmation-owned-shield.mjs'","from "+JSON.stringify(pathToFileURL(path.resolve('tests/reset-confirmation-owned-shield.mjs')).href));
replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
replace("roles:['stream_master']","roles:['admin','stream_admin','stream_master']");
replace("let cameraOnline=true;","let cameraOnline=true;\nconst heldAdminReplies=[];let actualAdminReads=0;");
replace("if(pathname==='/api/admin/users'||pathname==='/api/admin/users/create'){","if(pathname==='/api/admin/users'||pathname==='/api/admin/users/create'){\n    if(req.method==='GET'){actualAdminReads++;heldAdminReplies.push(res);return;}");
fixture+=fs.readFileSync('tests/neutral-owner-timeout-tail.js','utf8');
assert.equal(fs.readFileSync(sourcePath,'utf8'),original,'shared bootstrap bytes preserved');
await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));
