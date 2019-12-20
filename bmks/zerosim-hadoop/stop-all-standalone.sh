#!/bin/bash

jps

$HADOOP_HOME/bin/mapred --daemon stop historyserver

$SPARK_HOME/sbin/stop-all.sh
$HADOOP_HOME/sbin/stop-yarn.sh
$HADOOP_HOME/sbin/stop-dfs.sh

jps
