#!/bin/bash

jps

$HADOOP_HOME/sbin/start-dfs.sh
$HADOOP_HOME/sbin/start-yarn.sh

$HADOOP_HOME/bin/mapred --daemon start historyserver

$SPARK_HOME/sbin/start-master.sh -h localhost -p 7077
$SPARK_HOME/sbin/start-slave.sh localhost:707

jps
