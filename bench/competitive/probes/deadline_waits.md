# How late each timed wait returns past its deadline, and how soon a wake reaches it (AC-T7.4.1)

The record docs/design/deadline-waits.md decides from. `deadline_waits.c` times, for timeouts of 0.1, 1, 5, 25 and
100 ms, n = 100 waits of each OS mechanism nobody wakes (overshoot: return minus deadline) and n = 100 woken halfway
to their deadline by a spinning thread (latency: return minus the waker's call). The mechanisms are interleaved wait
by wait, so the host's load falls on each alike, and every wait loops on its deadline as Hex's sites do.
Percentiles are nearest-rank, in microseconds; the clock is Rust's `Instant`'s (`CLOCK_UPTIME_RAW` on macOS,
`CLOCK_MONOTONIC` on Linux). It ends with what a waiter's own wake object costs to make and close, 1,000 times.

On macOS: `cond_rel` is std's `Condvar` (`pthread_cond_timedwait_relative_np`), `dsema` std's `thread::park_timeout`
(`dispatch_semaphore_wait`), `kevent` a kqueue's wait with its timeout argument, `ktimer` a kqueue's wait for a one-shot
`EVFILT_TIMER` marked `NOTE_CRITICAL`, `poll` poll(2) on a pipe, `os_sync` `os_sync_wait_on_address_with_timeout`, and
`nanosleep` std's `thread::sleep` (woken by a signal, there being no other way). On Linux: `futex` is std's `Condvar`
(`FUTEX_WAIT_BITSET` to an absolute `CLOCK_MONOTONIC` deadline), `cond_mono` glibc's condition variable on that clock.

The source run is the file here, sha256 `44a8be107fb95283b9d457c922d37e151a93225682f18a197e54618d38f233e9`, built
`cc -O2 -Wall` (Linux: `-lpthread -lm`) and run `deadline_waits 100` as the kiln runner's command on each host, so at
the runner's QoS: `thread_qos=0x11` is `QOS_CLASS_UTILITY`.

## Run 1: 75694786 (MacBook Pro), 2026-11-12T19:41:38Z

Mac17,6, macOS 26.5.2 (Darwin 25.5.0), 18 cores, its user at it; load 7.19 to 6.64, other agents' builds beside it.

```
deadline_waits n=100 date=2026-11-12T19:41:38Z host=adalundhe-MacBook-Pro Darwin 25.5.0 arm64
model=Mac17,6 macOS=26.5.2 thread_qos=0x11
load before: 7.19 11.05 12.10
cond_rel  T=  0.1ms timeout n=100 over_us min 25 p50 406 p90 413 p99 826 max 1030 | wake n=100 woken=100 timedout=0 lat_us p50 10.2 p90 25.8 p99 415.4 max 479.9
dsema     T=  0.1ms timeout n=100 over_us min 26 p50 406 p90 413 p99 1837 max 2566 | wake n=100 woken=100 timedout=0 lat_us p50 8.7 p90 21.8 p99 88.0 max 270.6
kevent    T=  0.1ms timeout n=100 over_us min 11 p50 19 p90 42 p99 659 max 780 | wake n=100 woken=99 timedout=1 lat_us p50 9.1 p90 24.0 p99 391.8 max 391.8
ktimer    T=  0.1ms timeout n=100 over_us min 6 p50 17 p90 38 p99 702 max 2203 | wake n=100 woken=98 timedout=2 lat_us p50 8.8 p90 21.2 p99 251.0 max 251.0
poll      T=  0.1ms timeout n=100 over_us min 938 p50 1035 p90 1053 p99 1731 max 2413 | wake n=100 woken=100 timedout=0 lat_us p50 11.1 p90 24.8 p99 211.7 max 480.7
os_sync   T=  0.1ms timeout n=100 over_us min 16 p50 20 p90 33 p99 354 max 617 | wake n=100 woken=100 timedout=0 lat_us p50 8.9 p90 23.9 p99 231.0 max 544.7
nanosleep T=  0.1ms timeout n=100 over_us min 14 p50 407 p90 412 p99 1030 max 1793 | wake n=100 woken=99 timedout=1 lat_us p50 17.4 p90 33.7 p99 648.8 max 648.8
cond_rel  T=  1.0ms timeout n=100 over_us min 87 p50 2741 p90 4010 p99 4055 max 4103 | wake n=100 woken=100 timedout=0 lat_us p50 11.1 p90 333.2 p99 1217.0 max 1727.3
dsema     T=  1.0ms timeout n=100 over_us min 39 p50 3118 p90 4013 p99 4762 max 5158 | wake n=100 woken=100 timedout=0 lat_us p50 9.8 p90 347.5 p99 1002.1 max 1043.7
kevent    T=  1.0ms timeout n=100 over_us min 7 p50 133 p90 168 p99 564 max 621 | wake n=100 woken=99 timedout=1 lat_us p50 11.4 p90 139.5 p99 1155.6 max 1155.6
ktimer    T=  1.0ms timeout n=100 over_us min 9 p50 18 p90 165 p99 906 max 1255 | wake n=100 woken=98 timedout=2 lat_us p50 11.4 p90 70.3 p99 1728.9 max 1728.9
poll      T=  1.0ms timeout n=100 over_us min 11 p50 139 p90 158 p99 1010 max 1411 | wake n=100 woken=98 timedout=2 lat_us p50 15.3 p90 219.9 p99 1916.1 max 1916.1
os_sync   T=  1.0ms timeout n=100 over_us min 20 p50 135 p90 146 p99 562 max 571 | wake n=100 woken=100 timedout=0 lat_us p50 10.7 p90 102.0 p99 974.4 max 1502.8
nanosleep T=  1.0ms timeout n=100 over_us min 89 p50 2854 p90 4013 p99 4050 max 5539 | wake n=100 woken=100 timedout=0 lat_us p50 27.5 p90 203.2 p99 836.0 max 1580.6
cond_rel  T=  5.0ms timeout n=100 over_us min 47 p50 10884 p90 19830 p99 20025 max 20142 | wake n=100 woken=100 timedout=0 lat_us p50 12.5 p90 113.3 p99 872.6 max 1635.5
dsema     T=  5.0ms timeout n=100 over_us min 311 p50 11471 p90 20009 p99 20016 max 21029 | wake n=100 woken=100 timedout=0 lat_us p50 9.5 p90 233.0 p99 1169.0 max 1875.0
kevent    T=  5.0ms timeout n=100 over_us min 20 p50 632 p90 640 p99 2630 max 3134 | wake n=100 woken=100 timedout=0 lat_us p50 12.9 p90 134.0 p99 1107.8 max 1856.8
ktimer    T=  5.0ms timeout n=100 over_us min 10 p50 20 p90 76 p99 1803 max 2743 | wake n=100 woken=100 timedout=0 lat_us p50 12.4 p90 74.1 p99 1070.4 max 1117.2
poll      T=  5.0ms timeout n=100 over_us min 27 p50 637 p90 651 p99 1388 max 3419 | wake n=100 woken=100 timedout=0 lat_us p50 16.5 p90 74.8 p99 759.0 max 1233.6
os_sync   T=  5.0ms timeout n=100 over_us min 29 p50 635 p90 654 p99 2089 max 2631 | wake n=100 woken=100 timedout=0 lat_us p50 12.2 p90 38.0 p99 723.0 max 1635.3
nanosleep T=  5.0ms timeout n=100 over_us min 57 p50 12694 p90 19478 p99 20059 max 21672 | wake n=100 woken=100 timedout=0 lat_us p50 31.6 p90 48.5 p99 2890.0 max 3685.2
cond_rel  T= 25.0ms timeout n=100 over_us min 3438 p50 62359 p90 73938 p99 75005 max 77354 | wake n=100 woken=100 timedout=0 lat_us p50 8.1 p90 15.0 p99 2558.3 max 3706.7
dsema     T= 25.0ms timeout n=100 over_us min 158 p50 55999 p90 73604 p99 74981 max 74982 | wake n=100 woken=100 timedout=0 lat_us p50 7.8 p90 14.0 p99 2026.4 max 2922.2
kevent    T= 25.0ms timeout n=100 over_us min 9 p50 911 p90 1020 p99 2979 max 4944 | wake n=100 woken=100 timedout=0 lat_us p50 9.4 p90 21.4 p99 804.2 max 3257.5
ktimer    T= 25.0ms timeout n=100 over_us min 9 p50 22 p90 201 p99 1043 max 1203 | wake n=100 woken=100 timedout=0 lat_us p50 8.6 p90 20.2 p99 153.8 max 1029.3
poll      T= 25.0ms timeout n=100 over_us min 85 p50 1015 p90 1083 p99 2003 max 2022 | wake n=100 woken=100 timedout=0 lat_us p50 13.3 p90 20.8 p99 63.0 max 955.4
os_sync   T= 25.0ms timeout n=100 over_us min 15 p50 1008 p90 1024 p99 1606 max 2230 | wake n=100 woken=100 timedout=0 lat_us p50 7.5 p90 15.0 p99 233.3 max 237.4
nanosleep T= 25.0ms timeout n=100 over_us min 59 p50 58365 p90 74146 p99 74802 max 74862 | wake n=100 woken=100 timedout=0 lat_us p50 23.3 p90 45.3 p99 1656.5 max 2499.1
cond_rel  T=100.0ms timeout n=100 over_us min 1314 p50 63477 p90 73818 p99 74824 max 74918 | wake n=100 woken=100 timedout=0 lat_us p50 8.6 p90 12.5 p99 24.6 max 28.4
dsema     T=100.0ms timeout n=100 over_us min 120 p50 62608 p90 73654 p99 74474 max 74570 | wake n=100 woken=100 timedout=0 lat_us p50 9.4 p90 14.8 p99 35.5 max 253.0
kevent    T=100.0ms timeout n=100 over_us min 16 p50 752 p90 1011 p99 1017 max 1018 | wake n=100 woken=100 timedout=0 lat_us p50 10.5 p90 15.2 p99 23.1 max 35.6
ktimer    T=100.0ms timeout n=100 over_us min 10 p50 20 p90 28 p99 47 max 76 | wake n=100 woken=100 timedout=0 lat_us p50 10.3 p90 14.4 p99 24.9 max 37.4
poll      T=100.0ms timeout n=100 over_us min 22 p50 729 p90 1014 p99 1021 max 1031 | wake n=100 woken=100 timedout=0 lat_us p50 14.8 p90 24.8 p99 38.0 max 38.3
os_sync   T=100.0ms timeout n=100 over_us min 10 p50 741 p90 1011 p99 1033 max 1055 | wake n=100 woken=100 timedout=0 lat_us p50 9.2 p90 12.4 p99 20.5 max 20.8
nanosleep T=100.0ms timeout n=100 over_us min 295 p50 63523 p90 74339 p99 74979 max 74998 | wake n=100 woken=100 timedout=0 lat_us p50 23.8 p90 30.2 p99 70.4 max 87.6
make kqueue+user+close n=1000 us p50 0.71 p90 0.83 p99 2.33 max 17.88
make pipe+close n=1000 us p50 0.88 p90 1.58 p99 11.92 max 72.88
load after: 6.64 9.13 11.10
```

A wake run whose waiter `timedout` had its waker (a spinning thread) delayed past the waiter's deadline by the load.

## Run 2: 2768d0d5 (Mac Studio), 2026-11-12T19:36:27Z

Mac16,9, macOS 26.2 (Darwin 25.2.0), 14 cores, nobody at it (so XNU's user-idle level doubles every leeway); load
1.20 to 2.79, one CI job beside it.

```
deadline_waits n=100 date=2026-11-12T19:36:27Z host=Adas-Mac-Studio-4.local Darwin 25.2.0 arm64
model=Mac16,9 macOS=26.2 thread_qos=0x11
load before: 1.20 1.36 1.31
cond_rel  T=  0.1ms timeout n=100 over_us min 37 p50 809 p90 816 p99 864 max 872 | wake n=100 woken=100 timedout=0 lat_us p50 10.5 p90 15.2 p99 19.0 max 42.9
dsema     T=  0.1ms timeout n=100 over_us min 37 p50 809 p90 815 p99 842 max 883 | wake n=100 woken=100 timedout=0 lat_us p50 10.3 p90 14.3 p99 17.9 max 18.1
kevent    T=  0.1ms timeout n=100 over_us min 29 p50 35 p90 41 p99 66 max 91 | wake n=100 woken=100 timedout=0 lat_us p50 10.2 p90 15.3 p99 19.3 max 19.4
ktimer    T=  0.1ms timeout n=100 over_us min 10 p50 18 p90 25 p99 62 max 84 | wake n=100 woken=100 timedout=0 lat_us p50 10.0 p90 15.2 p99 17.4 max 22.7
poll      T=  0.1ms timeout n=100 over_us min 966 p50 1164 p90 1171 p99 1250 max 1707 | wake n=100 woken=100 timedout=0 lat_us p50 12.1 p90 18.3 p99 21.8 max 37.5
os_sync   T=  0.1ms timeout n=100 over_us min 18 p50 37 p90 42 p99 119 max 428 | wake n=100 woken=100 timedout=0 lat_us p50 10.3 p90 15.3 p99 21.4 max 27.2
nanosleep T=  0.1ms timeout n=100 over_us min 65 p50 809 p90 817 p99 827 max 838 | wake n=100 woken=100 timedout=0 lat_us p50 15.7 p90 26.2 p99 44.2 max 52.8
cond_rel  T=  1.0ms timeout n=100 over_us min 108 p50 5205 p90 8010 p99 8022 max 8079 | wake n=100 woken=100 timedout=0 lat_us p50 10.6 p90 14.0 p99 18.8 max 23.3
dsema     T=  1.0ms timeout n=100 over_us min 113 p50 5341 p90 8010 p99 8017 max 8058 | wake n=100 woken=100 timedout=0 lat_us p50 10.3 p90 13.5 p99 20.0 max 29.4
kevent    T=  1.0ms timeout n=100 over_us min 20 p50 262 p90 273 p99 542 max 599 | wake n=100 woken=100 timedout=0 lat_us p50 10.6 p90 15.5 p99 23.0 max 30.8
ktimer    T=  1.0ms timeout n=100 over_us min 8 p50 20 p90 30 p99 80 max 1572 | wake n=100 woken=100 timedout=0 lat_us p50 10.5 p90 14.0 p99 17.9 max 26.0
poll      T=  1.0ms timeout n=100 over_us min 35 p50 266 p90 273 p99 322 max 755 | wake n=100 woken=100 timedout=0 lat_us p50 10.7 p90 15.0 p99 23.7 max 23.7
os_sync   T=  1.0ms timeout n=100 over_us min 50 p50 263 p90 270 p99 321 max 328 | wake n=100 woken=100 timedout=0 lat_us p50 10.2 p90 13.4 p99 19.5 max 19.5
nanosleep T=  1.0ms timeout n=100 over_us min 136 p50 4220 p90 7670 p99 8019 max 8022 | wake n=100 woken=100 timedout=0 lat_us p50 14.0 p90 20.2 p99 39.8 max 67.4
cond_rel  T=  5.0ms timeout n=100 over_us min 2554 p50 29966 p90 38809 p99 40024 max 40029 | wake n=100 woken=100 timedout=0 lat_us p50 11.8 p90 14.5 p99 17.9 max 21.1
dsema     T=  5.0ms timeout n=100 over_us min 112 p50 30571 p90 38902 p99 40019 max 40032 | wake n=100 woken=100 timedout=0 lat_us p50 11.0 p90 14.1 p99 16.2 max 16.3
kevent    T=  5.0ms timeout n=100 over_us min 55 p50 1261 p90 1274 p99 1318 max 1850 | wake n=100 woken=100 timedout=0 lat_us p50 11.4 p90 13.5 p99 19.9 max 23.3
ktimer    T=  5.0ms timeout n=100 over_us min 12 p50 20 p90 40 p99 147 max 247 | wake n=100 woken=100 timedout=0 lat_us p50 11.8 p90 14.1 p99 17.2 max 33.1
poll      T=  5.0ms timeout n=100 over_us min 57 p50 1265 p90 1275 p99 1328 max 1471 | wake n=100 woken=100 timedout=0 lat_us p50 11.4 p90 13.8 p99 18.8 max 20.1
os_sync   T=  5.0ms timeout n=100 over_us min 174 p50 1265 p90 1278 p99 1335 max 1525 | wake n=100 woken=100 timedout=0 lat_us p50 11.5 p90 13.8 p99 15.5 max 16.7
nanosleep T=  5.0ms timeout n=100 over_us min 33 p50 29589 p90 38207 p99 40013 max 40072 | wake n=100 woken=100 timedout=0 lat_us p50 15.3 p90 18.2 p99 29.8 max 37.4
cond_rel  T= 25.0ms timeout n=100 over_us min 29 p50 56817 p90 144697 p99 150014 max 150020 | wake n=100 woken=100 timedout=0 lat_us p50 18.5 p90 148.5 p99 528.7 max 1176.2
dsema     T= 25.0ms timeout n=100 over_us min 909 p50 47156 p90 143002 p99 149302 max 150015 | wake n=100 woken=100 timedout=0 lat_us p50 15.7 p90 67.8 p99 637.1 max 1049.2
kevent    T= 25.0ms timeout n=100 over_us min 94 p50 2010 p90 2018 p99 2031 max 2145 | wake n=100 woken=100 timedout=0 lat_us p50 18.9 p90 103.2 p99 903.0 max 1383.4
ktimer    T= 25.0ms timeout n=100 over_us min 13 p50 24 p90 35 p99 535 max 786 | wake n=100 woken=100 timedout=0 lat_us p50 18.4 p90 79.0 p99 653.5 max 1714.0
poll      T= 25.0ms timeout n=100 over_us min 96 p50 2013 p90 2026 p99 2054 max 2336 | wake n=100 woken=100 timedout=0 lat_us p50 21.7 p90 60.7 p99 1075.6 max 7713.4
os_sync   T= 25.0ms timeout n=100 over_us min 46 p50 2011 p90 2020 p99 2105 max 2109 | wake n=100 woken=100 timedout=0 lat_us p50 15.6 p90 135.1 p99 383.4 max 448.0
nanosleep T= 25.0ms timeout n=100 over_us min 521 p50 25786 p90 137263 p99 148799 max 150009 | wake n=100 woken=100 timedout=0 lat_us p50 35.0 p90 184.3 p99 1974.0 max 2172.7
cond_rel  T=100.0ms timeout n=100 over_us min 231 p50 67454 p90 150013 p99 150017 max 150019 | wake n=100 woken=100 timedout=0 lat_us p50 12.1 p90 14.0 p99 16.2 max 16.8
dsema     T=100.0ms timeout n=100 over_us min 2380 p50 85132 p90 150012 p99 150015 max 150016 | wake n=100 woken=100 timedout=0 lat_us p50 11.7 p90 13.2 p99 16.8 max 19.9
kevent    T=100.0ms timeout n=100 over_us min 30 p50 2009 p90 2012 p99 2017 max 2030 | wake n=100 woken=100 timedout=0 lat_us p50 11.8 p90 14.4 p99 18.7 max 19.5
ktimer    T=100.0ms timeout n=100 over_us min 16 p50 25 p90 32 p99 66 max 90 | wake n=100 woken=100 timedout=0 lat_us p50 11.4 p90 13.7 p99 15.0 max 15.9
poll      T=100.0ms timeout n=100 over_us min 92 p50 2014 p90 2018 p99 2021 max 2021 | wake n=100 woken=100 timedout=0 lat_us p50 13.8 p90 16.4 p99 18.8 max 26.2
os_sync   T=100.0ms timeout n=100 over_us min 287 p50 2012 p90 2016 p99 2018 max 2020 | wake n=100 woken=100 timedout=0 lat_us p50 12.1 p90 13.8 p99 17.2 max 21.2
nanosleep T=100.0ms timeout n=100 over_us min 904 p50 50739 p90 147788 p99 150013 max 150016 | wake n=100 woken=100 timedout=0 lat_us p50 23.6 p90 26.5 p99 30.5 max 34.1
make kqueue+user+close n=1000 us p50 0.50 p90 0.54 p99 0.71 max 8.00
make pipe+close n=1000 us p50 0.67 p90 0.75 p99 0.96 max 4.92
load after: 2.79 2.37 1.77
```

## Run 3: 00aad8da (adacachy2), 2026-11-12T19:37:05Z

AMD Ryzen 9 9950X3D2 (32 threads), Linux 7.3.3-1-cachyos, the thread's timer slack 100 µs; load 10.07 to 6.30.

```
deadline_waits n=100 date=2026-11-12T19:37:05Z host=adacachy2 Linux 7.3.3-1-cachyos x86_64
timer_slack_ns=100000
load before: 10.07 8.88 7.34
futex     T=  0.1ms timeout n=100 over_us min 15 p50 101 p90 103 p99 104 max 108 | wake n=100 woken=100 timedout=0 lat_us p50 1.9 p90 3.0 p99 4.1 max 5.2
cond_mono T=  0.1ms timeout n=100 over_us min 3 p50 102 p90 102 p99 107 max 109 | wake n=100 woken=100 timedout=0 lat_us p50 2.0 p90 3.2 p99 5.0 max 5.5
poll      T=  0.1ms timeout n=100 over_us min 908 p50 1002 p90 1004 p99 1013 max 1093 | wake n=100 woken=100 timedout=0 lat_us p50 2.6 p90 6.6 p99 8.4 max 8.5
nanosleep T=  0.1ms timeout n=100 over_us min 9 p50 102 p90 103 p99 105 max 105 | wake n=100 woken=100 timedout=0 lat_us p50 3.4 p90 5.4 p99 9.3 max 10.5
futex     T=  1.0ms timeout n=100 over_us min 8 p50 102 p90 104 p99 107 max 118 | wake n=100 woken=100 timedout=0 lat_us p50 3.2 p90 4.9 p99 7.3 max 8.6
cond_mono T=  1.0ms timeout n=100 over_us min 8 p50 102 p90 104 p99 116 max 117 | wake n=100 woken=100 timedout=0 lat_us p50 3.5 p90 5.6 p99 10.5 max 11.5
poll      T=  1.0ms timeout n=100 over_us min 6 p50 102 p90 104 p99 108 max 239 | wake n=100 woken=100 timedout=0 lat_us p50 4.5 p90 8.3 p99 11.1 max 12.8
nanosleep T=  1.0ms timeout n=100 over_us min 9 p50 102 p90 104 p99 108 max 375 | wake n=100 woken=100 timedout=0 lat_us p50 6.0 p90 7.6 p99 10.9 max 11.1
futex     T=  5.0ms timeout n=100 over_us min 17 p50 103 p90 103 p99 107 max 108 | wake n=100 woken=100 timedout=0 lat_us p50 3.6 p90 5.3 p99 8.1 max 68.6
cond_mono T=  5.0ms timeout n=100 over_us min 11 p50 103 p90 104 p99 107 max 108 | wake n=100 woken=100 timedout=0 lat_us p50 4.3 p90 6.7 p99 9.9 max 10.8
poll      T=  5.0ms timeout n=100 over_us min 5 p50 103 p90 104 p99 106 max 106 | wake n=100 woken=100 timedout=0 lat_us p50 4.7 p90 8.5 p99 10.4 max 10.5
nanosleep T=  5.0ms timeout n=100 over_us min 19 p50 103 p90 104 p99 109 max 110 | wake n=100 woken=100 timedout=0 lat_us p50 7.4 p90 9.9 p99 35.2 max 292.1
futex     T= 25.0ms timeout n=100 over_us min 9 p50 103 p90 105 p99 108 max 108 | wake n=100 woken=100 timedout=0 lat_us p50 4.2 p90 6.5 p99 18.2 max 64.2
cond_mono T= 25.0ms timeout n=100 over_us min 7 p50 104 p90 106 p99 107 max 114 | wake n=100 woken=100 timedout=0 lat_us p50 4.9 p90 6.9 p99 15.1 max 18.0
poll      T= 25.0ms timeout n=100 over_us min 11 p50 104 p90 106 p99 108 max 108 | wake n=100 woken=100 timedout=0 lat_us p50 5.6 p90 9.6 p99 14.3 max 31.6
nanosleep T= 25.0ms timeout n=100 over_us min 4 p50 104 p90 106 p99 108 max 125 | wake n=100 woken=100 timedout=0 lat_us p50 7.8 p90 10.6 p99 31.9 max 50.3
futex     T=100.0ms timeout n=100 over_us min 10 p50 106 p90 108 p99 117 max 117 | wake n=100 woken=100 timedout=0 lat_us p50 5.1 p90 6.8 p99 11.3 max 20.6
cond_mono T=100.0ms timeout n=100 over_us min 12 p50 107 p90 109 p99 116 max 118 | wake n=100 woken=100 timedout=0 lat_us p50 5.7 p90 8.0 p99 11.8 max 28.0
poll      T=100.0ms timeout n=100 over_us min 16 p50 107 p90 109 p99 114 max 130 | wake n=100 woken=100 timedout=0 lat_us p50 6.9 p90 9.3 p99 11.8 max 12.4
nanosleep T=100.0ms timeout n=100 over_us min 11 p50 107 p90 109 p99 114 max 118 | wake n=100 woken=100 timedout=0 lat_us p50 8.9 p90 12.5 p99 21.0 max 22.8
make pipe+close n=1000 us p50 0.60 p90 0.73 p99 2.37 max 13.13
load after: 6.30 8.12 7.22
```
