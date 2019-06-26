#!/bin/bash
echo 2 > /proc/sys/kernel/randomize_va_space
echo never > /sys/kernel/mm/transparent_hugepage/enabled
./identity_map name testing pthread_ex 
./apriori_paging_set_process pthread_ex
./pthread_ex
#./identity_map name mcf_base.amd64-m64-gcc41-nn
#./apriori_paging_set_process mcf_base.amd64-m64-gcc41-nn
