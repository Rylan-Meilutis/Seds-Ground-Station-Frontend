//! Account management is a separate, explicitly authorized application area.
use super::{http_get_json,http_post_json};
use dioxus::prelude::*;
use serde::{Deserialize,Serialize};
#[derive(Clone,Default,Serialize,Deserialize)]
struct Permissions {view_data:bool,send_commands:bool,set_system_time:bool,voice_transmit:bool,#[serde(default)]camera_publish:bool}
#[derive(Clone,Default,Serialize,Deserialize)]
struct CommandAccess {allowed_commands:Vec<String>}
#[derive(Clone,Default,Serialize,Deserialize)]
struct CalibrationAccess {view:bool,edit:bool}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Account {
    username:String,roles:Vec<String>,permissions:Permissions,command_access:CommandAccess,
    calibration_access:CalibrationAccess,disabled:bool,
    #[serde(default,skip_serializing_if="Option::is_none")]
    password:Option<String>,
}
#[component]
pub(super) fn AdminConsole(on_exit:EventHandler<()>,on_navigate:EventHandler<String>)->Element {
    let mut accounts=use_signal(Vec::<Account>::new);
    let mut message=use_signal(String::new);
    let mut busy=use_signal(||false);
    let mut username=use_signal(String::new);
    let mut password=use_signal(String::new);
    let mut loaded=use_signal(||false);
    use_future(move||async move{
        match http_get_json::<Vec<Account>>("/api/admin/users").await {
            Ok(value)=>{accounts.set(value);loaded.set(true);},Err(error)=>message.set(error),
        }
    });
    let status=crate::auth::current_status();
    if !status.authenticated || !status.roles.iter().any(|role|role=="admin") {
        return rsx!{main {p {"Application administrator required"}button {onclick:move |_|on_exit.call(()),"Back to dashboard"}}};
    }
    rsx!{
        style {{include_str!("controls.css")}}
        style {{include_str!("admin_console.css")}}
        main {class:"gs26-admin","aria-label":"Administration",
            header {h1 {"Administration"}button {onclick:move |_|on_exit.call(()),"Back to settings"}}
            nav {"aria-label":"Administration tools",
                button {onclick:move |_|on_navigate.call("views".into()),"Dashboard views"}
            }
            super::station_positions_admin::StationPositionsAdmin {}
            p {role:"status","{message}"}
            section {h2 {"Users & access"}
                if !*loaded.read(){p {"Loading accounts…"}}
                for (index,account) in accounts.read().iter().enumerate() {
                    article {key:"{account.username}",
                        h3 {"{account.username}"}
                        div {class:"admin-permissions",
                            label {input {r#type:"checkbox",checked:account.roles.iter().any(|role|role=="admin"),disabled:*busy.read()||status.username.as_deref().is_some_and(|name|name.eq_ignore_ascii_case(&account.username)),onchange:move|e|{
                                let mut accounts=accounts.write();let roles=&mut accounts[index].roles;roles.retain(|role|role!="admin");if e.checked(){roles.push("admin".into());}
                            }}"Application administrator"}
                            label {input {r#type:"checkbox",checked:account.permissions.view_data,disabled:*busy.read(),onchange:move|e|accounts.write()[index].permissions.view_data=e.checked()}"View data"}
                            label {input {r#type:"checkbox",checked:account.permissions.send_commands,disabled:*busy.read(),onchange:move|e|accounts.write()[index].permissions.send_commands=e.checked()}"Send hardware commands"}
                            label {input {r#type:"checkbox",checked:account.permissions.camera_publish,disabled:*busy.read(),onchange:move|e|accounts.write()[index].permissions.camera_publish=e.checked()}"Share camera and microphone"}
                            label {input {r#type:"checkbox",checked:account.permissions.voice_transmit,disabled:*busy.read(),onchange:move|e|accounts.write()[index].permissions.voice_transmit=e.checked()}"Join and transmit crew voice"}
                            label {input {r#type:"checkbox",checked:account.permissions.set_system_time,disabled:*busy.read(),onchange:move|e|accounts.write()[index].permissions.set_system_time=e.checked()}"Set system time"}
                            label {input {r#type:"checkbox",checked:account.calibration_access.view,disabled:*busy.read(),onchange:move|e|accounts.write()[index].calibration_access.view=e.checked()}"View calibration"}
                            label {input {r#type:"checkbox",checked:account.calibration_access.edit,disabled:*busy.read(),onchange:move|e|accounts.write()[index].calibration_access.edit=e.checked()}"Edit calibration"}
                            label {input {r#type:"checkbox",checked:account.disabled,disabled:*busy.read()||status.username.as_deref().is_some_and(|name|name.eq_ignore_ascii_case(&account.username)),onchange:move|e|accounts.write()[index].disabled=e.checked()}"Disable account"}
                        }
                        details {summary {"Roles & command limits"}
                            div {class:"admin-permissions",
                                for (role,label) in [("stream_admin","Stream administrator"),("stream_master","Stream manager (manage cameras and broadcast)")] {
                                    label {input {r#type:"checkbox",checked:account.roles.iter().any(|value|value==role),disabled:*busy.read(),onchange:move|e|{
                                        let mut accounts=accounts.write();let roles=&mut accounts[index].roles;roles.retain(|value|value!=role);if e.checked(){roles.push(role.into());}
                                    }}"{label}"}
                                }
                            }
                            label {"New password (optional)" input {r#type:"password",value:account.password.clone().unwrap_or_default(),minlength:"12",maxlength:"128",autocomplete:"new-password",disabled:*busy.read(),oninput:move|e|accounts.write()[index].password=Some(e.value())}}
                            label {"Allowed commands" input {value:account.command_access.allowed_commands.join(", "),disabled:*busy.read(),oninput:move|e|accounts.write()[index].command_access.allowed_commands=e.value().split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect()}}
                            p {"Application admins inherit all permissions. Other accounts need explicit command access; an empty command list allows all commands."}
                        }
                        button {disabled:*busy.read(),onclick:move |_|{
                            let account=accounts.read()[index].clone();busy.set(true);
                            spawn(async move{
                                match http_post_json::<Account,Account>("/api/admin/users",&account).await {
                                    Ok(saved)=>{accounts.write()[index]=saved;message.set("Account saved".into());},Err(error)=>message.set(error),
                                }
                                busy.set(false);
                            });
                        },"Save {account.username}"}
                    }
                }
            }
            section {h2 {"Add account"}
                div {class:"admin-new",
                    label {"Username" input {value:username.read().clone(),maxlength:"64",autocomplete:"off",oninput:move|e|username.set(e.value())}}
                    label {"Temporary password" input {r#type:"password",value:password.read().clone(),minlength:"12",maxlength:"128",autocomplete:"new-password",oninput:move|e|password.set(e.value())}}
                    button {disabled:*busy.read()||username.read().trim().is_empty()||password.read().len()<12,onclick:move |_|{
                        let account=Account{username:username.read().trim().into(),password:Some(password.read().clone()),roles:vec!["stream_viewer".into()],permissions:Permissions{view_data:true,..Default::default()},..Default::default()};
                        busy.set(true);password.set(String::new());
                        spawn(async move{
                            match http_post_json::<Account,Account>("/api/admin/users/create",&account).await {
                                Ok(saved)=>{accounts.write().push(saved);username.set(String::new());message.set("Account created with viewing access".into());},Err(error)=>message.set(error),
                            }
                            busy.set(false);
                        });
                    },"Create viewer"}
                }
            }
        }
    }
}
