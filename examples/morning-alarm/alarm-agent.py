#!/usr/bin/env python3
import sys
import os
from datetime import datetime, timedelta

sys.stdout.reconfigure(encoding='utf-8')

# 计算到明天13点还有多少小时
now = datetime.now()
tomorrow_1pm = (now + timedelta(days=1)).replace(hour=13, minute=0, second=0)
hours_until = int((tomorrow_1pm - now).total_seconds() / 3600)

os.system(f'cryo-agent time "+{hours_until} hours"')

print("1")

os.system('cryo-agent hibernate')
