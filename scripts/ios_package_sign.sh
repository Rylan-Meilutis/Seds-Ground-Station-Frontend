#!/usr/bin/env bash
set -euo pipefail

# -----------------------------------------------------------------------------
# iOS App Signer (GUI) equivalent: optional embed.mobileprovision + sign + package .ipa
# - Does NOT build
# - Does NOT change bundle id
# - Does NOT change version strings
# - Applies get-task-allow from GET_TASK_ALLOW
# - Picks signing identity via regex from `security find-identity`
#
# Optional env:
#   CERT_REGEX='Apple Distribution:'   (default)
#   CERT_PICK='newest'|'first'         (default: newest)
#   GET_TASK_ALLOW='false'|'true'|'profile'|'none' (default: false)
#   WATCH_PROVISION=<path>             (required when the app embeds Watch/*.app)
#   WATCH_WIDGETS_PROVISION=<path>     (required when a watch app carries
#                                       PlugIns/GS26WatchWidgets.appex; build.py
#                                       removes the extension when it has none)
# -----------------------------------------------------------------------------

APP="${1:?Missing .app path}"
PROVISION="${2:?Missing .mobileprovision path, or '-' for no profile}"
IPA_OUT="${3:?Missing output .ipa path}"

CERT_REGEX="${CERT_REGEX:-Apple Distribution:}"
CERT_PICK="${CERT_PICK:-newest}"  # newest|first
GET_TASK_ALLOW="${GET_TASK_ALLOW:-false}"

PB="/usr/libexec/PlistBuddy"
CODESIGN="/usr/bin/codesign"
SECURITY="/usr/bin/security"
PLUTIL="/usr/bin/plutil"
OPENSSL="/usr/bin/openssl"

log() { printf "[%s] %s\n" "$(date '+%H:%M:%S')" "$*"; }
die() { printf "[ERROR] %s\n" "$*" >&2; exit 1; }

[[ -d "$APP" ]] || die "App bundle not found: $APP"
[[ -f "$APP/Info.plist" ]] || die "Missing Info.plist in app: $APP"
USE_PROVISION=1
if [[ "$PROVISION" == "-" || -z "$PROVISION" ]]; then
  USE_PROVISION=0
else
  [[ -f "$PROVISION" ]] || die "Provisioning profile not found: $PROVISION"
fi
[[ -x "$PB" ]] || die "PlistBuddy not found/executable at: $PB"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Decode provisioning profile once (used for cert matching + entitlements)
if [[ "$USE_PROVISION" -eq 1 ]]; then
  if ! $SECURITY cms -D -i "$PROVISION" > "$TMP/profile.plist" 2>/dev/null; then
    $OPENSSL smime -inform der -verify -noverify -in "$PROVISION" -out "$TMP/profile.plist" >/dev/null 2>&1 \
      || die "Failed to decode provisioning profile: $PROVISION"
  fi
fi

# -----------------------------------------------------------------------------
# Pick a signing identity from keychain WITHOUT hardcoding PII
# -----------------------------------------------------------------------------
identity_candidates="$($SECURITY find-identity -v -p codesigning \
  | sed -n 's/.*"\(.*\)".*/\1/p' \
  | grep -E "$CERT_REGEX" || true)"

all_identities="$($SECURITY find-identity -v -p codesigning \
  | sed -n 's/.*"\(.*\)".*/\1/p' || true)"

identity_hashes="$($SECURITY find-identity -v -p codesigning \
  | sed -n 's/.*\([0-9A-F]\{40\}\).*/\1/p' || true)"

profile_hashes=""
extract_profile_hashes() {
  $PLUTIL -extract DeveloperCertificates xml1 -o - "$TMP/profile.plist" 2>/dev/null \
    | awk '
        /<data>/{flag=1; next}
        /<\/data>/{flag=0; printf "\n"; next}
        { if (flag) { gsub(/^[ \t]+|[ \t]+$/, ""); printf "%s", $0 } }
      ' \
    | while IFS= read -r b64; do
        [[ -n "$b64" ]] || continue
        if printf "%s" "$b64" | base64 -D 2>/dev/null \
          | $OPENSSL x509 -inform DER -noout -fingerprint -sha1 2>/dev/null; then
          :
        else
          printf "%s" "$b64" | base64 -d 2>/dev/null \
            | $OPENSSL x509 -inform DER -noout -fingerprint -sha1 2>/dev/null || true
        fi \
          | sed 's/^SHA1 Fingerprint=//' \
          | tr -d ':' \
          | tr '[:lower:]' '[:upper:]'
      done
}

pick_identity_first() {
  printf "%s\n" "$identity_candidates" | head -n 1
}

# Picks the identity whose cert has the latest NotValidAfter.
# If cert parsing fails for all, returns empty (caller falls back to first).
pick_identity_newest() {
  # Print: epoch<TAB>name for each candidate, sort, take best name.
  printf "%s\n" "$identity_candidates" | while IFS= read -r name; do
    na="$($SECURITY find-certificate -c "$name" -p 2>/dev/null \
      | openssl x509 -noout -enddate 2>/dev/null \
      | head -n 1 \
      | sed 's/^notAfter=//')"

    epoch=0
    if [[ -n "${na:-}" ]]; then
      epoch="$(date -j -f "%b %e %T %Y %Z" "$na" "+%s" 2>/dev/null || echo 0)"
    fi

    printf "%s\t%s\n" "$epoch" "$name"
  done | sort -nr | head -n 1 | cut -f2- || true
}

IDENTITY=""
IDENTITY_HASH=""

if [[ "$USE_PROVISION" -eq 1 ]]; then
  log "Extracting allowed signing certs from provisioning profile..."
  profile_hashes="$(extract_profile_hashes || true)"
else
  log "No provisioning profile supplied; using certificate regex only."
fi

if [[ -n "$profile_hashes" ]]; then
  log "Provisioning profile specifies signing certificates; matching keychain identities..."
  matched_identities=""
  matched_hashes=""
  while IFS= read -r hash; do
    [[ -n "$hash" ]] || continue
    idx=0
    while IFS= read -r id_hash; do
      idx=$((idx + 1))
      if [[ "$id_hash" == "$hash" ]]; then
        name="$(printf "%s\n" "$all_identities" | sed -n "${idx}p")"
        if [[ -n "$name" ]]; then
          matched_identities="${matched_identities}${name}"$'\n'
          matched_hashes="${matched_hashes}${hash}"$'\n'
        fi
      fi
    done <<< "$identity_hashes"
  done <<< "$profile_hashes"

  identity_candidates="$(printf "%s" "$matched_identities" | sed '/^$/d' || true)"
  identity_hash_candidates="$(printf "%s" "$matched_hashes" | sed '/^$/d' || true)"
fi

[[ -n "$identity_candidates" ]] || {
  log "No signing identities matched the provisioning profile/cert regex."
  log "Available identities:"
  $SECURITY find-identity -v -p codesigning || true
  die "No matching signing identity found."
}

case "$CERT_PICK" in
  newest)
    IDENTITY="$(printf "%s\n" "$identity_candidates" | while IFS= read -r name; do
      na="$($SECURITY find-certificate -c "$name" -p 2>/dev/null \
        | openssl x509 -noout -enddate 2>/dev/null \
        | head -n 1 \
        | sed 's/^notAfter=//')"

      epoch=0
      if [[ -n "${na:-}" ]]; then
        epoch="$(date -j -f "%b %e %T %Y %Z" "$na" "+%s" 2>/dev/null || echo 0)"
      fi

      printf "%s\t%s\n" "$epoch" "$name"
    done | sort -nr | head -n 1 | cut -f2- || true)"
    if [[ -z "$IDENTITY" ]]; then
      IDENTITY="$(printf "%s\n" "$identity_candidates" | head -n 1)"
    fi
    ;;
  first)
    IDENTITY="$(printf "%s\n" "$identity_candidates" | head -n 1)"
    ;;
  *)
    die "CERT_PICK must be 'newest' or 'first' (got: $CERT_PICK)"
    ;;
esac

if [[ -n "${identity_hash_candidates:-}" ]]; then
  idx=0
  while IFS= read -r name; do
    idx=$((idx + 1))
    if [[ "$name" == "$IDENTITY" ]]; then
      IDENTITY_HASH="$(printf "%s\n" "$identity_hash_candidates" | sed -n "${idx}p")"
      break
    fi
  done <<< "$identity_candidates"
fi

if [[ -z "$IDENTITY_HASH" ]]; then
  # fallback: map by full list index
  idx=0
  while IFS= read -r name; do
    idx=$((idx + 1))
    if [[ "$name" == "$IDENTITY" ]]; then
      IDENTITY_HASH="$(printf "%s\n" "$identity_hashes" | sed -n "${idx}p")"
      break
    fi
  done <<< "$all_identities"
fi

[[ -n "$IDENTITY" ]] || die "Failed to pick a signing identity."

log "Using signing identity matching regex/profile (picked: $CERT_PICK)"
# (intentionally not printing the identity string to keep logs non-PII)
if [[ -n "$IDENTITY_HASH" ]]; then
  log "Using identity hash: ...${IDENTITY_HASH: -6}"
fi
# (intentionally not printing the identity string to keep logs non-PII)

# -----------------------------------------------------------------------------
# 1) Embed or remove provisioning profile
# -----------------------------------------------------------------------------
if [[ "$USE_PROVISION" -eq 1 ]]; then
  log "Embedding provisioning profile..."
  cp -f "$PROVISION" "$APP/embedded.mobileprovision"
else
  log "Removing embedded provisioning profile..."
  rm -f "$APP/embedded.mobileprovision" 2>/dev/null || true
fi

# -----------------------------------------------------------------------------
# 2) Remove old signatures (if any)
# -----------------------------------------------------------------------------
log "Removing old signature artifacts..."
rm -rf "$APP/_CodeSignature" 2>/dev/null || true
rm -f  "$APP/CodeResources" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 3) Extract entitlements
# -----------------------------------------------------------------------------
ENTITLEMENTS_PATH=""
if [[ "$USE_PROVISION" -eq 1 ]]; then
  log "Extracting entitlements from provisioning profile..."
  $PB -x -c "Print :Entitlements" "$TMP/profile.plist" > "$TMP/entitlements.plist"

  case "$GET_TASK_ALLOW" in
    false|true)
      if $PB -c "Print :get-task-allow" "$TMP/entitlements.plist" >/dev/null 2>&1; then
        $PB -c "Set :get-task-allow $GET_TASK_ALLOW" "$TMP/entitlements.plist" >/dev/null
      else
        $PB -c "Add :get-task-allow bool $GET_TASK_ALLOW" "$TMP/entitlements.plist" >/dev/null
      fi
      ;;
    profile|none)
      ;;
    *)
      die "GET_TASK_ALLOW must be 'false', 'true', 'profile', or 'none' (got: $GET_TASK_ALLOW)"
      ;;
  esac

  $PLUTIL -convert xml1 "$TMP/entitlements.plist" >/dev/null 2>&1 || true
  ENTITLEMENTS_PATH="$TMP/entitlements.plist"
else
  case "$GET_TASK_ALLOW" in
    none|false|profile)
      ;;
    *)
      die "GET_TASK_ALLOW must be 'none' when no provisioning profile is supplied (got: $GET_TASK_ALLOW)"
      ;;
  esac
fi

# -----------------------------------------------------------------------------
# 4) Sign nested content first (Frameworks, dylibs, appex)
# -----------------------------------------------------------------------------
sign_path() {
  local p="$1"
  log "Signing: $(basename "$p")"
  local signer="${IDENTITY_HASH:-$IDENTITY}"

  # Timestamp can fail offline; make it best-effort.
  # --options runtime is usually harmless; keep it.
  if ! $CODESIGN --force --sign "$signer" --options runtime --timestamp "$p" 2>/dev/null; then
    $CODESIGN --force --sign "$signer" --options runtime "$p"
  fi
}

if [[ -d "$APP/Frameworks" ]]; then
  find "$APP/Frameworks" -maxdepth 2 \( -name "*.framework" -o -name "*.dylib" \) -print0 \
    | while IFS= read -r -d '' item; do
        sign_path "$item"
      done
fi

# Signs one bundle with its own provisioning profile and that profile's
# entitlements (get-task-allow per GET_TASK_ALLOW).
sign_with_profile() {
  local bundle="$1" provision="$2" label="$3" tag
  tag="$(basename "$bundle" | tr -c 'A-Za-z0-9' '_')"
  if ! $SECURITY cms -D -i "$provision" > "$TMP/$tag-profile.plist" 2>/dev/null; then
    $OPENSSL smime -inform der -verify -noverify -in "$provision" -out "$TMP/$tag-profile.plist" >/dev/null 2>&1 \
      || die "Failed to decode $label provisioning profile: $provision"
  fi
  $PB -x -c "Print :Entitlements" "$TMP/$tag-profile.plist" > "$TMP/$tag-entitlements.plist"
  case "$GET_TASK_ALLOW" in
    false|true)
      if $PB -c "Print :get-task-allow" "$TMP/$tag-entitlements.plist" >/dev/null 2>&1; then
        $PB -c "Set :get-task-allow $GET_TASK_ALLOW" "$TMP/$tag-entitlements.plist" >/dev/null
      else
        $PB -c "Add :get-task-allow bool $GET_TASK_ALLOW" "$TMP/$tag-entitlements.plist" >/dev/null
      fi
      ;;
  esac
  $PLUTIL -convert xml1 "$TMP/$tag-entitlements.plist" >/dev/null 2>&1 || true
  cp -f "$provision" "$bundle/embedded.mobileprovision"
  rm -rf "$bundle/_CodeSignature" 2>/dev/null || true
  log "Signing $label: $(basename "$bundle")"
  local signer="${IDENTITY_HASH:-$IDENTITY}"
  if ! $CODESIGN --force --sign "$signer" --entitlements "$TMP/$tag-entitlements.plist" \
    --options runtime --timestamp "$bundle" 2>/dev/null; then
    $CODESIGN --force --sign "$signer" --entitlements "$TMP/$tag-entitlements.plist" \
      --options runtime "$bundle"
  fi
}

# The watch's push-to-talk widget needs its own profile; any other appex is
# signed plainly as before.
sign_appex() {
  local appex="$1"
  if [[ "$(basename "$appex")" == "GS26WatchWidgets.appex" ]]; then
    [[ -n "${WATCH_WIDGETS_PROVISION:-}" && -f "$WATCH_WIDGETS_PROVISION" ]] \
      || die "Watch widget $(basename "$appex") needs WATCH_WIDGETS_PROVISION (a profile for its bundle id)."
    sign_with_profile "$appex" "$WATCH_WIDGETS_PROVISION" "watch widget"
  else
    sign_path "$appex"
  fi
}

find "$APP" -maxdepth 2 -name "*.appex" -print0 2>/dev/null \
  | while IFS= read -r -d '' appex; do
      sign_appex "$appex"
    done

# -----------------------------------------------------------------------------
# 4b) Sign an embedded Apple Watch app with its own profile and entitlements
# -----------------------------------------------------------------------------
for watch_app in "$APP"/Watch/*.app; do
  [[ -d "$watch_app" ]] || continue
  [[ -n "${WATCH_PROVISION:-}" && -f "$WATCH_PROVISION" ]] \
    || die "Embedded watch app $(basename "$watch_app") needs WATCH_PROVISION (a profile for its bundle id)."
  if ! $SECURITY cms -D -i "$WATCH_PROVISION" > "$TMP/watch-profile.plist" 2>/dev/null; then
    $OPENSSL smime -inform der -verify -noverify -in "$WATCH_PROVISION" -out "$TMP/watch-profile.plist" >/dev/null 2>&1 \
      || die "Failed to decode watch provisioning profile: $WATCH_PROVISION"
  fi
  $PB -x -c "Print :Entitlements" "$TMP/watch-profile.plist" > "$TMP/watch-entitlements.plist"
  case "$GET_TASK_ALLOW" in
    false|true)
      if $PB -c "Print :get-task-allow" "$TMP/watch-entitlements.plist" >/dev/null 2>&1; then
        $PB -c "Set :get-task-allow $GET_TASK_ALLOW" "$TMP/watch-entitlements.plist" >/dev/null
      else
        $PB -c "Add :get-task-allow bool $GET_TASK_ALLOW" "$TMP/watch-entitlements.plist" >/dev/null
      fi
      ;;
  esac
  $PLUTIL -convert xml1 "$TMP/watch-entitlements.plist" >/dev/null 2>&1 || true
  cp -f "$WATCH_PROVISION" "$watch_app/embedded.mobileprovision"
  rm -rf "$watch_app/_CodeSignature" 2>/dev/null || true
  if [[ -d "$watch_app/Frameworks" ]]; then
    find "$watch_app/Frameworks" -maxdepth 2 \( -name "*.framework" -o -name "*.dylib" \) -print0 \
      | while IFS= read -r -d '' item; do
          sign_path "$item"
        done
  fi
  for appex in "$watch_app"/PlugIns/*.appex; do
    [[ -d "$appex" ]] || continue
    sign_appex "$appex"
  done
  log "Signing embedded watch app: $(basename "$watch_app")"
  signer="${IDENTITY_HASH:-$IDENTITY}"
  if ! $CODESIGN --force --sign "$signer" --entitlements "$TMP/watch-entitlements.plist" \
    --options runtime --timestamp "$watch_app" 2>/dev/null; then
    $CODESIGN --force --sign "$signer" --entitlements "$TMP/watch-entitlements.plist" \
      --options runtime "$watch_app"
  fi
done

# -----------------------------------------------------------------------------
# 5) Sign the main app with entitlements
# -----------------------------------------------------------------------------
log "Signing main app bundle..."
signer="${IDENTITY_HASH:-$IDENTITY}"
if [[ -n "$ENTITLEMENTS_PATH" ]]; then
  if ! $CODESIGN --force --sign "$signer" \
    --entitlements "$ENTITLEMENTS_PATH" \
    --options runtime --timestamp \
    "$APP" 2>/dev/null; then
    $CODESIGN --force --sign "$signer" \
      --entitlements "$ENTITLEMENTS_PATH" \
      --options runtime \
      "$APP"
  fi
else
  if ! $CODESIGN --force --sign "$signer" \
    --options runtime --timestamp \
    "$APP" 2>/dev/null; then
    $CODESIGN --force --sign "$signer" \
      --options runtime \
      "$APP"
  fi
fi

# -----------------------------------------------------------------------------
# 6) Verify
# -----------------------------------------------------------------------------
log "Verifying signature..."
$CODESIGN --verify --deep --strict --verbose=2 "$APP"

# -----------------------------------------------------------------------------
# 7) Package IPA (Payload/*.app)
# -----------------------------------------------------------------------------
log "Packaging IPA..."
rm -f "$IPA_OUT" 2>/dev/null || true
mkdir -p "$TMP/Payload"
cp -R "$APP" "$TMP/Payload/"

(
  cd "$TMP"
  /usr/bin/zip -qry "$IPA_OUT" Payload
)

log "✅ Done: $IPA_OUT"
