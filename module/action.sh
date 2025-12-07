#!/system/bin/sh

MODDIR=${0%/*}
FREE_FILE="$MODDIR/free"
AUTO_FILE="$MODDIR/auto"

if [ -f "$FREE_FILE" ]; then
    FREE_VALUE=$(cat "$FREE_FILE" | tr -d '[:space:]')
else
    FREE_VALUE="0"
fi

AUTO_EXISTS=0
if [ -f "$AUTO_FILE" ]; then
    AUTO_EXISTS=1
fi

# 三种状态循环切换
if [ "$FREE_VALUE" = "0" ] && [ "$AUTO_EXISTS" = "0" ]; then
    # 状态1: free=0, 无auto → 状态2: free=1, 无auto
    printf "1" > "$FREE_FILE"
    echo "锁定PPS支持"
elif [ "$FREE_VALUE" = "1" ] && [ "$AUTO_EXISTS" = "0" ]; then
    # 状态2: free=1, 无auto → 状态3: free=1, 有auto
    touch "$AUTO_FILE"
    echo "开启协议自动识别"
else
    # 状态3: free=1, 有auto → 状态1: free=0, 无auto
    printf "0" > "$FREE_FILE"
    rm -f "$AUTO_FILE"
    echo "关闭PPS支持"
fi

sleep 0.3
sleep 0.27
