#!/usr/bin/env python3

import os
import subprocess
import sys
import shutil
import re

def get_version_from_cargo_toml(cargo_toml_path):
    """从 Cargo.toml 文件中提取版本号"""
    try:
        with open(cargo_toml_path, 'r', encoding='utf-8') as f:
            content = f.read()
            # 使用正则表达式匹配版本号
            version_match = re.search(r'version\s*=\s*"([^"]+)"', content)
            if version_match:
                return version_match.group(1)
            else:
                print("警告：无法在 Cargo.toml 中找到版本号，将使用默认文件名 FreePPS.zip")
                return ""
    except Exception as e:
        print(f"警告：读取 Cargo.toml 文件时出错: {e}，将使用默认文件名 FreePPS.zip")
        return ""

def package_module():
    """打包module文件夹为zip文件"""
    print("开始打包module文件夹...")
    
    # 获取项目根目录
    project_root = os.path.dirname(os.path.abspath(__file__))
    
    # 获取版本号
    cargo_toml_path = os.path.join(project_root, "Cargo.toml")
    version = get_version_from_cargo_toml(cargo_toml_path)
    
    # 定义路径
    output_dir = os.path.join(project_root, "output")
    module_dir = os.path.join(project_root, "module")
    seven_zip_path = r"D:\7-Zip\7z.exe"
    
    # 检查FreePPS文件是否存在（使用output目录下的文件）
    free_pps_path = os.path.join(output_dir, "FreePPS")
    if not os.path.exists(free_pps_path):
        print(f"错误：找不到FreePPS文件: {free_pps_path}")
        print("请确保output目录下存在编译好的FreePPS文件")
        sys.exit(1)
    
    # 检查7-ZIP是否存在
    if not os.path.exists(seven_zip_path):
        print(f"错误：找不到7-ZIP程序: {seven_zip_path}")
        print("请确保7-ZIP已安装在D:\7-Zip目录下")
        sys.exit(1)
    
    # 步骤1: 将FreePPS复制到module\bin目录
    print("步骤1: 将FreePPS复制到module\\bin目录...")
    module_bin_dir = os.path.join(module_dir, "bin")
    target_path = os.path.join(module_bin_dir, "FreePPS")
    
    # 确保bin目录存在
    os.makedirs(module_bin_dir, exist_ok=True)
    
    # 复制文件
    shutil.copy2(free_pps_path, target_path)
    print(f"✓ 已将FreePPS复制到: {target_path}")
    
    # 步骤2: 使用7-ZIP压缩整个module文件夹
    print("步骤2: 使用7-ZIP压缩module文件夹...")
    # 根据版本号确定文件名
    if version:
        zip_filename = f"FreePPS_v{version}.zip"
        print(f"检测到项目版本号: {version}")
    else:
        zip_filename = "FreePPS.zip"
        print("未检测到项目版本号，使用默认文件名")
    
    zip_file_path = os.path.join(project_root, zip_filename)
    
    # 如果已存在zip文件，先删除
    if os.path.exists(zip_file_path):
        os.remove(zip_file_path)
        print(f"已删除现有的{zip_filename}文件")
    
    # 使用7-ZIP压缩
    try:
        # 构建7-ZIP命令
        # a: 添加文件到压缩包
        # -tzip: 使用zip格式
        # -r: 递归子目录
        cmd = [
            seven_zip_path,
            "a",
            "-tzip",
            "-r",
            zip_file_path,
            f"{module_dir}\\*"
        ]
        
        print(f"执行命令: {' '.join(cmd)}")
        result = subprocess.run(cmd, capture_output=True, text=True, cwd=project_root)
        
        if result.returncode != 0:
            print(f"7-ZIP压缩失败: {result.stderr}")
            sys.exit(1)
        
        print("✓ 压缩完成")
        
    except Exception as e:
        print(f"压缩过程出错: {e}")
        sys.exit(1)
    
    # 步骤3: 将压缩包移到output文件夹
    print("步骤3: 将压缩包移到output文件夹...")
    
    # 确保output文件夹存在
    os.makedirs(output_dir, exist_ok=True)
    
    final_zip_path = os.path.join(output_dir, zip_filename)
    
    # 如果output文件夹中已存在，先删除
    if os.path.exists(final_zip_path):
        os.remove(final_zip_path)
    
    # 移动文件
    shutil.move(zip_file_path, final_zip_path)
    print(f"✓ 已将压缩包移动到: {final_zip_path}")
    
    # 显示最终文件大小
    file_size = os.path.getsize(final_zip_path)
    file_size_mb = file_size / (1024 * 1024)
    print(f"\n打包完成！")
    print(f"压缩包大小: {file_size_mb:.2f} MB")
    print(f"输出路径: {final_zip_path}")

if __name__ == "__main__":
    try:
        package_module()
        print("\n所有步骤完成！✓")
    except KeyboardInterrupt:
        print("\n操作被用户中断")
        sys.exit(1)
    except Exception as e:
        print(f"\n发生错误: {e}")
        sys.exit(1)