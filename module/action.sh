#!/system/bin/sh

MODDIR=${0%/*}
FREE_FILE="$MODDIR/free"

if [ -f "$FREE_FILE" ]; then
    FREE_VALUE=$(cat "$FREE_FILE" | tr -d '[:space:]')
else
    FREE_VALUE="0"
fi

if [ "$FREE_VALUE" = "0" ]; then
    printf "1" > "$FREE_FILE"
    echo "锁定PPS支持"
else
    printf "0" > "$FREE_FILE"
    echo "关闭PPS支持"
fi

sleep 0.3
sleep 0.27
