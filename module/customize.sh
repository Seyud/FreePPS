#!/system/bin/sh

MODDIR=${0%/*}

# Function to detect key press (Volume Up or Volume Down) or timeout
detect_key_press() {
    timeout_seconds=6

    read -r -t $timeout_seconds line < <(getevent -ql | awk '/KEY_VOLUME/ {print; exit}')

    if [ $? -eq 142 ]; then
        echo "[!] No key pressed within $timeout_seconds seconds. Skipping installation..."
        return 1
    fi

    if echo "$line" | grep -q "KEY_VOLUMEDOWN"; then
        return 0
    else
        echo "[+] Skipping installation..."
        return 1
    fi
}

# Installation prompt if FreePPS app is not installed
pm path me.freepps.tile > /dev/null 2>&1 || {
    echo "[+] FreePPS App"
    echo "[?] Do you want to install FreePPS App"
    echo "[?] VOL [+]: NO"
    echo "[?] VOL [-]: YES"
    if detect_key_press; then
        echo "[+] Installing FreePPS App..."
        chmod +x "$MODPATH/freepps-app.sh"
        sh "$MODPATH/freepps-app.sh"
    fi
    rm -f "$MODPATH/freepps-app.sh"
}

# Set permissions
set_perm_recursive $MODPATH 0 0 0755 0644
set_perm $MODPATH/bin/FreePPS 0 0 0755
