#!/system/bin/sh

MODDIR=${0%/*}

# 定义free文件路径
FREE_FILE="$MODDIR/free"

# 检查free文件是否存在
if [ -f "$FREE_FILE" ]; then
    # 读取当前内容
    CURRENT=$(cat "$FREE_FILE")
    
    # 切换0和1
    if [ "$CURRENT" = "0" ]; then
        echo "1" > "$FREE_FILE"
        echo "已切换为PPS"
    elif [ "$CURRENT" = "1" ]; then
        echo "0" > "$FREE_FILE"
        echo "已切换为MIPPS"
    else
        # 如果内容不是0或1，默认设置为1
        echo "1" > "$FREE_FILE"
        echo "内容无效，已重置为PPS"
    fi
else
    # 如果文件不存在，创建并设置为1
    echo "1" > "$FREE_FILE"
    echo "文件不存在，已创建并设置为PPS"
fi

sleep 0.3
sleep 0.27

sleep 1