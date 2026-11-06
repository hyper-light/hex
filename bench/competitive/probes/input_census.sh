#!/usr/bin/bash
# The input census (bench/competitive/probes/input_census.py) on this host: $1 is the probe, $2 the log.
P="$1"
O="$2"
{
  date -u
  uname -r
  uptime
  echo "pid_max $(cat /proc/sys/kernel/pid_max)"
  echo '== cgroup'
  nice -n 10 python3 -I "$P" cgroup
  echo '== net host'
  python3 -I "$P" net
  echo '== net fresh'
  unshare -rn /usr/bin/bash -c "ip link set lo up && python3 -I '$P' net"
  echo "exit $?"
} > "$O" 2>&1
