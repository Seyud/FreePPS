#!/system/bin/sh
MODDIR=${0%/*}
FREE_FILE="$MODDIR/free"
if [ ! -f "$FREE_FILE" ]; then
    echo "1" > "$FREE_FILE"
    echo "文件不存在，已创建并开启PPS支持"
else
    CURRENT=$(<"$FREE_FILE")
    case "$CURRENT" in
        0)
            echo "1" > "$FREE_FILE"
            echo "开启PPS支持"
            ;;
        1)
            echo "0" > "$FREE_FILE"
            echo "关闭PPS支持"
            ;;
        *)
            echo "1" > "$FREE_FILE"
            echo "内容无效，已重置为PPS支持"
            ;;
    esac
fi
sleep 0.3
sleep 0.27
sleep 1