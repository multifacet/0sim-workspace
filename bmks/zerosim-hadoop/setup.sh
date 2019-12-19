#!/bin/bash

# format hdfs namenode
$HADOOP_HOME/bin/hdfs namenode -format -force -finalize

# create needed directories
$HADOOP_HOME/sbin/start-all.sh
$HADOOP_HOME/bin/hdfs dfs -mkdir /home
$HADOOP_HOME/bin/hdfs dfs -mkdir /home/vagrant
$HADOOP_HOME/sbin/stop-all.sh

# build HiBench
$HIBENCH_HOME/bin/build_all.sh
