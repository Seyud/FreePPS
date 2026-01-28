#!/bin/sh
PATH=/data/adb/ap/bin:/data/adb/ksu/bin:/data/adb/magisk:/data/data/com.termux/files/usr/bin:$PATH

# APP package name
APP_PACKAGE="me.freepps.tile"

# APK download URL
APK_URL="https://gitee.com/Seyud/FreePPS_app/releases/download/v1.0.0/FreePPS_app.apk"

# Temporary directory to store the APK
TEMP_DIR="/data/local/tmp/freepps-app"
APK_PATH="$TEMP_DIR/FreePPS_app.apk"

# Create necessary directories
mkdir -p "$TEMP_DIR"

echo "[+] Downloading FreePPS App from Gitee..."
echo "[+] URL: $APK_URL"

# Try curl first, then wget
if command -v curl > /dev/null 2>&1; then
    echo "[+] Using curl to download..."
    MAX_RETRIES=3
    RETRY_DELAY=2
    RETRY_COUNT=0
    
    while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
        RETRY_COUNT=$((RETRY_COUNT + 1))
        if [ $RETRY_COUNT -gt 1 ]; then
            echo "[+] Retry $RETRY_COUNT/$MAX_RETRIES after ${RETRY_DELAY}s..."
            sleep $RETRY_DELAY
        fi
        
        echo "[+] Attempt $RETRY_COUNT/$MAX_RETRIES..."
        curl -L \
            --connect-timeout 15 \
            --max-time 60 \
            -A "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36" \
            -e "https://gitee.com/Seyud/FreePPS_app/releases" \
            -# \
            -o "$APK_PATH" \
            "$APK_URL" 2>&1
        
        CURL_EXIT=$?
        
        if [ $CURL_EXIT -eq 0 ]; then
            FILE_SIZE=$(stat -c%s "$APK_PATH" 2>/dev/null || stat -f%z "$APK_PATH" 2>/dev/null)
            if [ "$FILE_SIZE" -gt 1000 ]; then
                echo "[+] Download successful on attempt $RETRY_COUNT"
                break
            fi
        fi
        
        if [ $RETRY_COUNT -lt $MAX_RETRIES ]; then
            echo "[x] Download failed (exit code: $CURL_EXIT), retrying..."
            rm -f "$APK_PATH"
        fi
    done
    
    if [ $RETRY_COUNT -ge $MAX_RETRIES ]; then
        echo "[x] Download failed after $MAX_RETRIES attempts"
        rm -f "$APK_PATH"
    fi
else
    echo "[+] Using wget to download..."
    busybox wget -T 15 --no-check-certificate -O "$APK_PATH" "$APK_URL" 2>&1
fi

# Check if download was successful
if [ ! -f "$APK_PATH" ]; then
    echo "[x] Downloaded file not found."
    rm -rf "$TEMP_DIR"
    exit 1
fi

FILE_SIZE=$(stat -c%s "$APK_PATH" 2>/dev/null || stat -f%z "$APK_PATH" 2>/dev/null)
if [ "$FILE_SIZE" -lt 1000 ] 2>/dev/null; then
    echo "[x] Downloaded file is too small ($FILE_SIZE bytes), likely an error page."
    echo "[x] File content:"
    head -c 500 "$APK_PATH"
    echo ""
    rm -rf "$TEMP_DIR"
    exit 1
fi

echo "[+] APK downloaded successfully. Size: $FILE_SIZE bytes"

# Install the APK as a user app
echo "[+] Installing APK..."
pm install -r "$APK_PATH" 2>&1 </dev/null | cat

# Check if the installation was successful by verifying the app's presence
if pm path "$APP_PACKAGE" > /dev/null 2>&1; then
    echo "[+] APK installed successfully as a user app."
    echo "[+] FreePPS App Installed"
else
    echo "[x] Failed to install apk."
    # Save the APK to the failsafe directory
    mkdir -p /sdcard/Download/FreePPS_app
    cp -f "$APK_PATH" /sdcard/Download/FreePPS_app/FreePPS_app.apk
    echo "[*] Please manually install app from /sdcard/Download/FreePPS_app/FreePPS_app.apk"
fi

# Clean up
rm -rf "$TEMP_DIR"
