//! Account management is a separate, explicitly authorized application area.
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct Permissions {
    view_data: bool,
    send_commands: bool,
    set_system_time: bool,
    voice_transmit: bool,
    #[serde(default)]
    camera_publish: bool,
    #[serde(default)]
    sign_checklists: bool,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct CommandAccess {
    allowed_commands: Vec<String>,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct CalibrationAccess {
    view: bool,
    edit: bool,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct Account {
    username: String,
    roles: Vec<String>,
    permissions: Permissions,
    command_access: CommandAccess,
    calibration_access: CalibrationAccess,
    disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}
#[derive(Serialize)]
struct DeleteRequest {
    username: String,
}
#[derive(Deserialize)]
struct Deleted {
    #[allow(dead_code)]
    deleted: String,
}

const ADMIN: &str = "admin";
/// Stream roles in increasing authority; an account shows the strongest one.
const STREAM_ROLES: [(&str, &str, &str); 4] = [
    ("", "No stream role", "Uses only the permissions chosen below."),
    ("stream_viewer", "Stream viewer", "Watches the program feed; can never direct the stream."),
    ("stream_master", "Stream manager", "Switches cameras and runs the broadcast."),
    ("stream_admin", "Stream administrator", "Manages the stream and chooses stream managers."),
];
/// Per-permission toggles offered to non-administrator accounts.
const PERMISSION_TOGGLES: [(&str, &str); 6] = [
    ("view_data", "View telemetry and station data"),
    ("send_commands", "Send hardware commands"),
    ("sign_checklists", "Sign off checklists"),
    ("voice_transmit", "Join and talk on crew voice"),
    ("camera_publish", "Share camera and microphone"),
    ("set_system_time", "Set station system time"),
];
/// Choices when creating an account; each is a starting point that can be edited.
const NEW_ACCOUNT_KINDS: [(&str, &str, &str); 3] = [
    ("viewer", "Viewer", "Can see data only."),
    ("operator", "Operator", "Can see data and send hardware commands."),
    ("admin", "Administrator", "Every permission, always, plus user management."),
];

fn is_admin(account: &Account) -> bool {
    account.roles.iter().any(|role| role == ADMIN)
}
fn stream_role(account: &Account) -> &'static str {
    STREAM_ROLES
        .iter()
        .rev()
        .find(|(role, _, _)| !role.is_empty() && account.roles.iter().any(|r| r == role))
        .map(|(role, _, _)| *role)
        .unwrap_or("")
}
fn permission(account: &Account, name: &str) -> bool {
    let p = &account.permissions;
    match name {
        "view_data" => p.view_data || p.send_commands,
        "send_commands" => p.send_commands,
        "set_system_time" => p.set_system_time,
        "voice_transmit" => p.voice_transmit,
        "camera_publish" => p.camera_publish,
        "sign_checklists" => p.sign_checklists,
        _ => false,
    }
}
fn set_permission(account: &mut Account, name: &str, value: bool) {
    let p = &mut account.permissions;
    match name {
        "view_data" => {
            p.view_data = value;
            // Commands always imply viewing; revoking view revokes commands.
            if !value {
                p.send_commands = false;
            }
        }
        "send_commands" => {
            p.send_commands = value;
            if value {
                p.view_data = true;
            }
        }
        "set_system_time" => p.set_system_time = value,
        "voice_transmit" => p.voice_transmit = value,
        "camera_publish" => p.camera_publish = value,
        "sign_checklists" => p.sign_checklists = value,
        _ => {}
    }
}
fn set_admin(account: &mut Account, admin: bool) {
    account.roles.retain(|role| role != ADMIN);
    if admin {
        account.roles.retain(|role| role != "stream_viewer");
        account.roles.push(ADMIN.into());
    }
}
fn set_stream_role(account: &mut Account, role: &str) {
    account
        .roles
        .retain(|r| !STREAM_ROLES.iter().any(|(stream, _, _)| !stream.is_empty() && r == stream));
    if !role.is_empty() {
        account.roles.push(role.into());
    }
}
fn role_label(account: &Account) -> &'static str {
    if is_admin(account) {
        return "Administrator";
    }
    match stream_role(account) {
        "stream_admin" => "Stream administrator",
        "stream_master" => "Stream manager",
        "stream_viewer" if !account.permissions.send_commands => "Viewer",
        _ if account.permissions.send_commands => "Operator",
        _ if account.permissions.view_data => "Viewer",
        _ => "No access",
    }
}
fn access_summary(account: &Account) -> String {
    if is_admin(account) {
        return "All permissions".into();
    }
    let granted: Vec<_> = PERMISSION_TOGGLES
        .iter()
        .filter(|(name, _)| permission(account, name))
        .map(|(_, label)| *label)
        .collect();
    if granted.is_empty() {
        "No permissions".into()
    } else {
        granted.join(" · ")
    }
}
fn same_user(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
fn enabled_admins(accounts: &[Account]) -> usize {
    accounts.iter().filter(|a| !a.disabled && is_admin(a)).count()
}
/// Why the current admin may not demote, disable or delete this account.
fn protected_reason(saved: &[Account], account: &str, me: Option<&str>) -> Option<&'static str> {
    if me.is_some_and(|me| same_user(me, account)) {
        return Some("This is your account. You can't remove your own administrator access, disable it or delete it.");
    }
    let saved_account = saved.iter().find(|a| same_user(&a.username, account))?;
    (is_admin(saved_account) && !saved_account.disabled && enabled_admins(saved) <= 1)
        .then_some("This is the last enabled administrator. Make another account an administrator first.")
}
/// Changes that take access away from someone and so need confirmation.
fn destructive_changes(before: Option<&Account>, after: &Account) -> Vec<String> {
    let Some(before) = before else { return Vec::new() };
    let mut changes = Vec::new();
    if is_admin(before) && !is_admin(after) {
        changes.push("remove administrator access".into());
    }
    if !before.disabled && after.disabled {
        changes.push("disable the account and sign it out".into());
    }
    if after.password.as_ref().is_some_and(|p| !p.is_empty()) {
        changes.push("reset the password and sign out every existing session".into());
    }
    if !is_admin(after) {
        let revoked: Vec<_> = PERMISSION_TOGGLES
            .iter()
            .filter(|(name, _)| (is_admin(before) || permission(before, name)) && !permission(after, name))
            .map(|(_, label)| label.to_lowercase())
            .collect();
        if !revoked.is_empty() && !(is_admin(before) && !is_admin(after)) {
            changes.push(format!("revoke: {}", revoked.join(", ")));
        }
    }
    changes
}
fn validation_error(account: &Account) -> Option<String> {
    let password = account.password.as_deref().unwrap_or("");
    if !password.is_empty() && !(12..=128).contains(&password.chars().count()) {
        return Some(format!("The new password for {} must be 12–128 characters.", account.username));
    }
    if account.command_access.allowed_commands.len() > 128 {
        return Some("Allow at most 128 commands.".into());
    }
    None
}
fn matches_filter(account: &Account, query: &str, filter: &str) -> bool {
    let query = query.trim().to_lowercase();
    (query.is_empty() || account.username.to_lowercase().contains(&query))
        && match filter {
            "admin" => is_admin(account),
            "custom" => !is_admin(account),
            "active" => !account.disabled,
            "disabled" => account.disabled,
            _ => true,
        }
}
/// Server errors arrive as `HTTP 409: reason`; show the reason with its code.
fn server_error(action: &str, error: String) -> String {
    format!("Could not {action}. {error}")
}

#[derive(Clone, PartialEq)]
enum Pending {
    Save { username: String, changes: Vec<String> },
    Delete { username: String },
}
/// Search and filters appear once the list is long enough to need them.
const FILTER_THRESHOLD: usize = 6;

#[component]
pub(super) fn AdminConsole(on_exit: EventHandler<()>, on_navigate: EventHandler<String>) -> Element {
    let authority = neutral_request_owner::use_factory();
    // `accounts` holds editable drafts; `saved` is the last server copy.
    let mut accounts = use_signal(Vec::<Account>::new);
    let mut saved = use_signal(Vec::<Account>::new);
    let mut message = use_signal(String::new);
    let mut failed = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut new_kind = use_signal(|| "viewer".to_string());
    let mut loaded = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut filter = use_signal(|| "all".to_string());
    let mut pending = use_signal(|| None::<Pending>);
    #[cfg(all(test, not(target_arch = "wasm32")))]
    super::neutral_request_owner::native_tests::observe_admin_signals(move || (*busy.peek(), message.peek().clone()));
    use_future(move || {
        authority.run(async move {
            match authority.get::<Vec<Account>>("/api/admin/users").await {
                Ok(value) => {
                    saved.set(value.clone());
                    accounts.set(value);
                    loaded.set(true);
                }
                Err(error) => {
                    failed.set(true);
                    message.set(server_error("load accounts", error));
                }
            }
        })
    });
    let status = crate::auth::current_status();
    if !status.authenticated || !status.roles.iter().any(|role| role == ADMIN) {
        return rsx! {main {p {"Application administrator required"}button {onclick:move |_|on_exit.call(()),"Back to dashboard"}}};
    }
    if !authority.current() {
        return rsx! {main {{neutral_request_owner::retired_notice()} button {onclick:move |_|on_exit.call(()),"Back to settings"}}};
    }
    let me = status.username.clone();
    let report = move |text: String, error: bool| {
        let (mut failed, mut message) = (failed, message);
        failed.set(error);
        message.set(text);
    };
    let save = move |name: String| {
        let (mut busy, mut pending) = (busy, pending);
        let Some(index) = accounts.read().iter().position(|a| same_user(&a.username, &name)) else { return };
        let account = accounts.read()[index].clone();
        if let Some(error) = validation_error(&account) {
            report(error, true);
            return;
        }
        if !authority.current() {
            return;
        }
        busy.set(true);
        pending.set(None);
        authority.spawn(async move {
            let (mut accounts, mut saved, mut busy) = (accounts, saved, busy);
            match authority.post::<Account, Account>("/api/admin/users", &account).await {
                Ok(stored) => {
                    let name = stored.username.clone();
                    for list in [&mut accounts, &mut saved] {
                        let mut list = list.write();
                        if let Some(slot) = list.iter_mut().find(|a| same_user(&a.username, &name)) {
                            *slot = stored.clone();
                        }
                    }
                    report("Account saved".into(), false);
                }
                Err(error) => report(server_error(&format!("save {}", account.username), error), true),
            }
            busy.set(false);
        });
    };
    let delete = move |name: String| {
        let (mut busy, mut pending) = (busy, pending);
        if !authority.current() {
            return;
        }
        busy.set(true);
        pending.set(None);
        authority.spawn(async move {
            let (mut accounts, mut saved, mut busy) = (accounts, saved, busy);
            let body = DeleteRequest { username: name.clone() };
            match authority.post::<DeleteRequest, Deleted>("/api/admin/users/delete", &body).await {
                Ok(_) => {
                    accounts.write().retain(|a| !same_user(&a.username, &name));
                    saved.write().retain(|a| !same_user(&a.username, &name));
                    report(format!("Deleted {name}"), false);
                }
                Err(error) => report(server_error(&format!("delete {name}"), error), true),
            }
            busy.set(false);
        });
    };
    let is_busy = *busy.read();
    let saved_list = saved.read().clone();
    let total = accounts.read().len();
    let active = accounts.read().iter().filter(|a| !a.disabled).count();
    let show_filters = total > FILTER_THRESHOLD;
    let visible: Vec<(usize, Account)> = accounts
        .read()
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, a)| !show_filters || matches_filter(a, &query.read(), &filter.read()))
        .collect();
    let new_name = username.read().trim().to_string();
    let new_password_len = password.read().chars().count();
    let duplicate = accounts.read().iter().any(|a| same_user(&a.username, &new_name));
    let create_problem = if new_name.is_empty() {
        None
    } else if duplicate {
        Some("An account with that username already exists.")
    } else if new_name.chars().count() > 64 || new_name.chars().any(char::is_control) {
        Some("Usernames are 1–64 characters.")
    } else if new_password_len > 0 && !(12..=128).contains(&new_password_len) {
        Some("Temporary passwords are 12–128 characters.")
    } else {
        None
    };
    let can_create = !is_busy && !new_name.is_empty() && create_problem.is_none() && (12..=128).contains(&new_password_len);
    rsx! {
        style {{include_str!("controls.css")}}
        style {{include_str!("admin_console.css")}}
        main {class:"gs26-admin","aria-label":"Administration",
            header {h1 {"Administration"}button {onclick:move |_|on_exit.call(()),"Back to settings"}}
            nav {"aria-label":"Administration tools",
                button {onclick:move |_|on_navigate.call("views".into()),"Dashboard views"}
            }
            super::station_positions_admin::StationPositionsAdmin {}
            super::station_pairing_admin::StationPairingAdmin {}
            super::sequence_access_admin::SequenceAccessAdmin {}
            super::physical_panel_admin::PhysicalPanelAdmin {}
            if *failed.read() {
                p {class:"admin-message admin-error",role:"alert","{message}"}
            } else {
                p {class:"admin-message",role:"status","{message}"}
            }
            section {class:"admin-users","aria-labelledby":"admin-users-title",
                div {class:"admin-users-head",
                    h2 {id:"admin-users-title","Users & access"}
                    if *loaded.read() {span {class:"admin-count","{total} accounts · {active} active"}}
                }
                p {class:"admin-hint","Administrators always have every permission. Other accounts get exactly the permissions ticked for them."}
                if show_filters {
                    div {class:"admin-filters",
                        label {"Search accounts" input {r#type:"search",value:"{query}",placeholder:"Name",autocomplete:"off",oninput:move |e|query.set(e.value())}}
                        label {"Show" select {value:"{filter}",onchange:move |e|filter.set(e.value()),
                            option {value:"all","All accounts"}
                            option {value:"admin","Administrators"}
                            option {value:"custom","Other roles"}
                            option {value:"active","Active"}
                            option {value:"disabled","Disabled"}
                        }}
                    }
                }
                if !*loaded.read() && !*failed.read() {p {"Loading accounts…"}}
                if *loaded.read() && visible.is_empty() {p {"No accounts match."}}
                ul {class:"admin-user-list",
                    for (index,account) in visible {
                        {
                            let name = account.username.clone();
                            let admin = is_admin(&account);
                            let mine = me.as_deref().is_some_and(|me| same_user(me, &name));
                            let protected = protected_reason(&saved_list, &name, me.as_deref());
                            let saved_copy = saved_list.iter().find(|a| same_user(&a.username, &name)).cloned();
                            let dirty = saved_copy.as_ref() != Some(&account);
                            let status_label = if account.disabled {"Disabled"} else {"Active"};
                            let stream = stream_role(&account);
                            let (save_name, cancel_name, delete_name) = (name.clone(), name.clone(), name.clone());
                            rsx! {
                                li {key:"{name}",class:if account.disabled {"admin-user is-disabled"} else {"admin-user"},
                                    div {class:"admin-user-summary",
                                        div {class:"admin-user-name",
                                            h3 {"{name}"}
                                            if mine {span {class:"admin-badge","You"}}
                                            span {class:if admin {"admin-badge is-admin"} else {"admin-badge"},"{role_label(&account)}"}
                                            span {class:if account.disabled {"admin-badge is-off"} else {"admin-badge is-on"},"{status_label}"}
                                            if dirty {span {class:"admin-badge is-dirty","Unsaved changes"}}
                                        }
                                        p {class:"admin-user-access","{access_summary(&account)}"}
                                    }
                                    details {class:"admin-editor",
                                        summary {"Edit {name}"}
                                        fieldset {class:"admin-roles",disabled:is_busy,
                                            legend {"Role"}
                                            label {class:"admin-choice",
                                                input {r#type:"radio",name:"role-{name}",checked:admin,disabled:is_busy,onchange:move |_|set_admin(&mut accounts.write()[index], true)}
                                                span {strong {"Administrator"} " Every permission, always, including user management. Cannot be narrowed."}
                                            }
                                            label {class:"admin-choice",
                                                input {r#type:"radio",name:"role-{name}",checked:!admin,disabled:is_busy||(admin&&protected.is_some()),onchange:move |_|set_admin(&mut accounts.write()[index], false)}
                                                span {strong {"Custom access"} " Only the permissions chosen below."}
                                            }
                                            if admin && protected.is_some() {p {class:"admin-note","{protected.unwrap_or_default()}"}}
                                        }
                                        if admin {
                                            div {class:"admin-locked",role:"note",
                                                strong {"All permissions"}
                                                " Administrators hold every permission on this station and paired stations, including any added later. Hardware commands from a recovery station still go through the primary's checks."
                                            }
                                        } else {
                                            fieldset {class:"admin-roles",disabled:is_busy,
                                                legend {"Stream role"}
                                                for (role,label,description) in STREAM_ROLES {
                                                    label {class:"admin-choice",
                                                        input {r#type:"radio",name:"stream-{name}",checked:stream==role,disabled:is_busy,onchange:move |_|set_stream_role(&mut accounts.write()[index], role)}
                                                        span {strong {"{label}"} " {description}"}
                                                    }
                                                }
                                            }
                                            fieldset {class:"admin-permissions",disabled:is_busy,
                                                legend {"Permissions"}
                                                for (perm,label) in PERMISSION_TOGGLES {
                                                    label {input {r#type:"checkbox",checked:permission(&account, perm),disabled:is_busy,onchange:move |e|set_permission(&mut accounts.write()[index], perm, e.checked())}"{label}"}
                                                }
                                                label {input {r#type:"checkbox",checked:account.calibration_access.view||account.calibration_access.edit,disabled:is_busy,onchange:move |e|{let mut list=accounts.write();let c=&mut list[index].calibration_access;c.view=e.checked();if !e.checked(){c.edit=false;}}}"View calibration"}
                                                label {input {r#type:"checkbox",checked:account.calibration_access.edit,disabled:is_busy,onchange:move |e|{let mut list=accounts.write();let c=&mut list[index].calibration_access;c.edit=e.checked();if e.checked(){c.view=true;}}}"Edit calibration"}
                                            }
                                            label {class:"admin-field","Allowed commands (comma separated; empty allows every command)"
                                                input {value:account.command_access.allowed_commands.join(", "),disabled:is_busy,autocomplete:"off",oninput:move |e|accounts.write()[index].command_access.allowed_commands=e.value().split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect()}
                                            }
                                        }
                                        fieldset {class:"admin-permissions",disabled:is_busy,
                                            legend {"Account"}
                                            label {input {r#type:"checkbox",checked:account.disabled,disabled:is_busy||(protected.is_some()&&!account.disabled),onchange:move |e|accounts.write()[index].disabled=e.checked()}"Disable sign-in"}
                                            if mine {p {class:"admin-note","{protected.unwrap_or_default()}"}}
                                        }
                                        label {class:"admin-field","Reset password (optional, 12–128 characters)"
                                            input {r#type:"password",value:account.password.clone().unwrap_or_default(),minlength:"12",maxlength:"128",autocomplete:"new-password",disabled:is_busy,oninput:move |e|accounts.write()[index].password=Some(e.value())}
                                        }
                                        div {class:"admin-actions",
                                            button {class:"admin-primary",disabled:is_busy,onclick:move |_|{
                                                let name=save_name.clone();
                                                let before=saved.read().iter().find(|a|same_user(&a.username,&name)).cloned();
                                                let after=accounts.read().iter().find(|a|same_user(&a.username,&name)).cloned();
                                                let Some(after)=after else {return};
                                                let changes=destructive_changes(before.as_ref(), &after);
                                                if changes.is_empty() {save(name);} else {pending.set(Some(Pending::Save{username:name,changes}));}
                                            },"Save {name}"}
                                            button {disabled:is_busy||!dirty,onclick:move |_|{
                                                let name=cancel_name.clone();
                                                let original=saved.read().iter().find(|a|same_user(&a.username,&name)).cloned();
                                                if let Some(original)=original && let Some(slot)=accounts.write().iter_mut().find(|a|same_user(&a.username,&name)) {*slot=original;}
                                            },"Discard changes"}
                                            button {class:"admin-danger",disabled:is_busy||protected.is_some(),title:protected.unwrap_or("Delete this account"),onclick:move |_|pending.set(Some(Pending::Delete{username:delete_name.clone()})),"Delete {name}"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            section {class:"admin-add","aria-labelledby":"admin-add-title",
                details {
                    summary {h2 {id:"admin-add-title","Add user"}}
                    div {class:"admin-new",
                        label {class:"admin-field","Username" input {value:username.read().clone(),maxlength:"64",autocomplete:"off",oninput:move |e|username.set(e.value())}}
                        label {class:"admin-field","Temporary password" input {r#type:"password",value:password.read().clone(),minlength:"12",maxlength:"128",autocomplete:"new-password",oninput:move |e|password.set(e.value())}}
                        fieldset {class:"admin-roles",
                            legend {"Starting role"}
                            for (kind,label,description) in NEW_ACCOUNT_KINDS {
                                label {class:"admin-choice",
                                    input {r#type:"radio",name:"new-account-kind",checked:*new_kind.read()==kind,onchange:move |_|new_kind.set(kind.into())}
                                    span {strong {"{label}"} " {description}"}
                                }
                            }
                        }
                        if let Some(problem)=create_problem {p {class:"admin-note admin-error",role:"alert","{problem}"}}
                        p {class:"admin-hint","Share the temporary password privately. You can fine-tune permissions after creating the account."}
                        button {class:"admin-primary",disabled:!can_create,onclick:move |_|{
                            let kind=new_kind.read().clone();
                            let mut account=Account{username:username.read().trim().into(),password:Some(password.read().clone()),..Default::default()};
                            match kind.as_str() {
                                "admin"=>{account.roles=vec![ADMIN.into()];account.permissions.view_data=true;}
                                "operator"=>{account.permissions.view_data=true;account.permissions.send_commands=true;}
                                _=>{account.roles=vec!["stream_viewer".into()];account.permissions.view_data=true;}
                            }
                            if !authority.current(){return;}busy.set(true);password.set(String::new());
                            authority.spawn(async move{
                                match authority.post::<Account,Account>("/api/admin/users/create",&account).await {
                                    Ok(created)=>{
                                        let name=created.username.clone();
                                        accounts.write().push(created.clone());saved.write().push(created);
                                        username.set(String::new());report(format!("Created {name}"),false);
                                    }
                                    Err(error)=>report(server_error(&format!("create {}",account.username),error),true),
                                }
                                busy.set(false);
                            });
                        },"Create account"}
                    }
                }
            }
            if let Some(action)=pending.read().clone() {
                div {class:"gs26-modal admin-confirm",role:"alertdialog","aria-modal":"true","aria-label":"Confirm account change",
                    {match action.clone() {
                        Pending::Save{username:name,changes}=>rsx!{
                            h2 {"Confirm changes to {name}"}
                            p {"Saving will:"}
                            ul {for change in changes {li {"{change}"}}}
                            div {class:"admin-actions",
                                button {class:"gs26-modal-danger",disabled:is_busy,onclick:move |_|save(name.clone()),"Confirm and save"}
                                button {disabled:is_busy,onclick:move |_|pending.set(None),"Keep editing"}
                            }
                        },
                        Pending::Delete{username:name}=>rsx!{
                            h2 {"Delete {name}?"}
                            p {"The account is removed permanently and every session it has ends immediately. This cannot be undone; disabling sign-in is reversible."}
                            div {class:"admin-actions",
                                button {class:"gs26-modal-danger",disabled:is_busy,onclick:move |_|delete(name.clone()),"Delete permanently"}
                                button {disabled:is_busy,onclick:move |_|pending.set(None),"Cancel"}
                            }
                        },
                    }}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn account(name: &str, roles: &[&str]) -> Account {
        Account {
            username: name.into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            permissions: Permissions { view_data: true, ..Default::default() },
            ..Default::default()
        }
    }
    #[test]
    fn self_and_last_enabled_admin_are_protected() {
        let list = vec![account("alice", &["admin"]), account("bob", &[])];
        assert!(protected_reason(&list, "ALICE", Some("alice")).is_some());
        assert!(protected_reason(&list, "alice", Some("carol")).is_some());
        assert!(protected_reason(&list, "bob", Some("alice")).is_none());
        let mut list = list;
        list[1].roles = vec!["admin".into()];
        assert!(protected_reason(&list, "bob", Some("alice")).is_none());
        list[1].disabled = true;
        assert!(protected_reason(&list, "alice", Some("carol")).is_some());
    }
    #[test]
    fn removing_access_needs_confirmation_but_granting_does_not() {
        let before = account("bob", &["admin"]);
        let mut after = before.clone();
        assert!(destructive_changes(Some(&before), &after).is_empty());
        set_admin(&mut after, false);
        assert_eq!(destructive_changes(Some(&before), &after), vec!["remove administrator access"]);
        let before = account("bob", &[]);
        let mut after = before.clone();
        set_permission(&mut after, "send_commands", true);
        assert!(destructive_changes(Some(&before), &after).is_empty());
        set_permission(&mut after, "view_data", false);
        assert!(!after.permissions.send_commands);
        assert_eq!(destructive_changes(Some(&before), &after).len(), 1);
        after = before.clone();
        after.disabled = true;
        after.password = Some("a fresh password".into());
        assert_eq!(destructive_changes(Some(&before), &after).len(), 2);
    }
    #[test]
    fn admin_summary_is_locked_to_all_permissions() {
        let mut admin = account("alice", &["admin", "stream_viewer"]);
        admin.permissions = Permissions::default();
        assert_eq!(access_summary(&admin), "All permissions");
        assert_eq!(role_label(&admin), "Administrator");
        set_admin(&mut admin, true);
        assert!(!admin.roles.iter().any(|r| r == "stream_viewer"));
    }
    #[test]
    fn stream_role_choice_is_exclusive_and_keeps_admin() {
        let mut a = account("bob", &["stream_master", "stream_admin"]);
        assert_eq!(stream_role(&a), "stream_admin");
        set_stream_role(&mut a, "stream_viewer");
        assert_eq!(a.roles, vec!["stream_viewer"]);
        set_stream_role(&mut a, "");
        assert!(a.roles.is_empty());
    }
    #[test]
    fn filters_match_name_role_and_status() {
        let mut a = account("Flight-Director", &["admin"]);
        assert!(matches_filter(&a, "director", "admin"));
        assert!(!matches_filter(&a, "", "custom"));
        a.disabled = true;
        assert!(matches_filter(&a, "", "disabled"));
        assert!(!matches_filter(&a, "", "active"));
    }
    #[test]
    fn short_reset_passwords_are_rejected_before_sending() {
        let mut a = account("bob", &[]);
        a.password = Some("short".into());
        assert!(validation_error(&a).is_some());
        a.password = Some(String::new());
        assert!(validation_error(&a).is_none());
    }
}
