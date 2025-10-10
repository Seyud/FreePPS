#!/system/bin/sh

MODDIR=${0%/*}

FREE_FILE="$MODDIR/free"

if [ -f "$FREE_FILE" ]; then
    CURRENT=$(cat "$FREE_FILE")
    
    if [ "$CURRENT" = "0" ]; then
        echo "1" > "$FREE_FILE"
        echo "已切换为PPS"
    elif [ "$CURRENT" = "1" ]; then
        echo "0" > "$FREE_FILE"
        echo "已切换为MIPPS"
    else
        echo "1" > "$FREE_FILE"
        echo "内容无效，已重置为PPS"
    fi
else
    echo "1" > "$FREE_FILE"
    echo "文件不存在，已创建并设置为PPS"
fi

sleep 0.3
sleep 0.27

sleep 1