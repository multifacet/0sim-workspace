#!/bin/bash

# format hdfs namenode
$HADOOP_HOME/bin/hdfs namenode -format -force -finalize

# create needed directories
$HADOOP_HOME/sbin/start-all.sh
$HADOOP_HOME/bin/hdfs dfs -mkdir /user
$HADOOP_HOME/bin/hdfs dfs -mkdir /user/markm
$HADOOP_HOME/sbin/stop-all.sh

# build HiBench
$HIBENCH_HOME/bin/build_all.sh
