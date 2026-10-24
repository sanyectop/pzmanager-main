import arepy_steamworks
import inspect

# 查看包路径
print("包路径:", arepy_steamworks.__file__)

# 列出包内所有公开的名称
print("\n包内可用名称:")
for name in dir(arepy_steamworks):
    if not name.startswith('_'):
        print(" -", name)