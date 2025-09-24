#!/system/bin/sh

# 定义文件路径变量
PD_VERIFED_FILE="/sys/class/qcom-battery/pd_verifed"

# 检查 pd_verifed 文件是否存在
if [ ! -e "$PD_VERIFED_FILE" ]; then
    abort "pd_verifed 文件不存在，模块安装失败"
fi

set_perm_recursive $MODPATH 0 0 0755 0644
set_perm $MODPATH/bin/FreePPS 0 0 0755


