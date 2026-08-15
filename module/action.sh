#!/system/bin/sh

MODDIR=${0%/*}
FREE_FILE="$MODDIR/free"
AUTO_FILE="$MODDIR/auto"

if [ -f "$FREE_FILE" ]; then
    FREE_VALUE=$(cat "$FREE_FILE" | tr -d '[:space:]')
else
    FREE_VALUE="0"
fi

if [ "$FREE_VALUE" = "0" ]; then
    rm -f "$AUTO_FILE"
    printf "1" > "$FREE_FILE"
    echo "✅锁定PPS支持⚡"
elif [ ! -f "$AUTO_FILE" ]; then
    touch "$AUTO_FILE"
    echo "🔄协议自动识别💡"
else
    printf "0" > "$FREE_FILE"
    rm -f "$AUTO_FILE"
    echo "⏸️小米协议优先💤"
fi

sleep 0.3
sleep 0.27
