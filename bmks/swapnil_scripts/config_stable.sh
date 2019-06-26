#!/bin/bash
echo 2 > /proc/sys/kernel/randomize_va_space
echo never > /sys/kernel/mm/transparent_hugepage/enabled
./identity_map name stable check_mapping_stable 
./apriori_paging_set_process check_mapping_stable
./check_mapping_stable
#./identity_map name mcf_base.amd64-m64-gcc41-nn
#./apriori_paging_set_process mcf_base.amd64-m64-gcc41-nn
